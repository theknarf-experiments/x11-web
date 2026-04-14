/**
 * WebRTC peer connection hook for direct sidecar ↔ frontend communication.
 *
 * Establishes a WebRTC data channel (binary msgpack) for display updates and
 * input events, plus audio tracks for sound playback and microphone.
 *
 * Signaling flows through the existing WebSocket via the backend relay.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { decode, encode } from "@msgpack/msgpack";
import type {
	BackendToFrontend,
	DisplayUpdate,
	FrontendToBackend,
	InputEvent,
} from "./types";

/** Data channel message from sidecar (mirrors Rust DcServerMsg). */
type DcServerMsg =
	| { t: "d"; c: string; u: DisplayUpdate }
	| { t: "cb"; selection: string; mime_type: string; data: Uint8Array }
	| { t: "co"; selection: string; mime_types: string[] }
	| { t: "pc"; pid: number; client_id: string; command: string }
	| { t: "pe"; pid: number; exit_code: number | null }
	| { t: "id"; window_id: string; reason: string };

/** Data channel message to sidecar (mirrors Rust DcClientMsg). */
type DcClientMsg =
	| { t: "i"; w: string; e: InputEvent }
	| { t: "r"; window_id: string }
	| { t: "rw"; window_id: string; width: number; height: number }
	| { t: "rs"; width: number; height: number }
	| { t: "sc"; selection: string; mime_type: string; data: Uint8Array }
	| { t: "rc"; selection: string; mime_type: string }
	| { t: "sp"; request_id: string; command: string; args: string[] }
	| { t: "kp"; request_id: string; pid: number };

export type DisplayUpdateCallback = (
	sidecarId: string,
	clientId: string,
	update: DisplayUpdate,
) => void;

export interface UseWebRTCOptions {
	/** Send a signaling message via the existing WebSocket. */
	sendWs: (msg: FrontendToBackend) => void;
	/** Callback for display updates received over the data channel. */
	onDisplayUpdate?: DisplayUpdateCallback;
	/** Callback for process connected events. */
	onProcessConnected?: (
		pid: number,
		clientId: string,
		command: string,
	) => void;
	/** Callback for process exited events. */
	onProcessExited?: (pid: number, exitCode: number | null) => void;
}

export interface WebRTCHandle {
	/** Whether the data channel is open and ready. */
	connected: boolean;
	/** Handle an incoming signaling message from the backend WS. */
	handleSignaling: (msg: BackendToFrontend) => void;
	/** Initiate a WebRTC connection to a sidecar. */
	connect: (sidecarId: string) => void;
	/** Send an input event over the data channel. */
	sendInput: (windowId: string, event: InputEvent) => void;
	/** Send a resize window command over the data channel. */
	sendResizeWindow: (
		windowId: string,
		width: number,
		height: number,
	) => void;
	/** Send a resize screen command over the data channel. */
	sendResizeScreen: (width: number, height: number) => void;
	/** Send a redraw request over the data channel. */
	sendRedraw: (windowId: string) => void;
	/** Spawn a process via data channel. */
	sendSpawn: (requestId: string, command: string, args: string[]) => void;
	/** Kill a process via data channel. */
	sendKill: (requestId: string, pid: number) => void;
}

export function useWebRTC(options: UseWebRTCOptions): WebRTCHandle {
	const pcRef = useRef<RTCPeerConnection | null>(null);
	const dcRef = useRef<RTCDataChannel | null>(null);
	const sidecarIdRef = useRef<string>("");
	const optionsRef = useRef(options);
	optionsRef.current = options;
	const [connected, setConnected] = useState(false);

	// Clean up on unmount.
	useEffect(() => {
		return () => {
			dcRef.current?.close();
			pcRef.current?.close();
		};
	}, []);

	const sendDc = useCallback((msg: DcClientMsg) => {
		const dc = dcRef.current;
		if (dc && dc.readyState === "open") {
			dc.send(encode(msg) as Uint8Array);
		}
	}, []);

	const handleSignaling = useCallback(
		(msg: BackendToFrontend) => {
			if (msg.type === "RtcOffer") {
				handleOffer(msg.sidecar_id, msg.sdp);
			} else if (msg.type === "RtcIceCandidate") {
				const pc = pcRef.current;
				if (pc) {
					pc.addIceCandidate(
						new RTCIceCandidate({
							candidate: msg.candidate,
							sdpMid: msg.sdp_mid ?? undefined,
							sdpMLineIndex: msg.sdp_mline_index ?? undefined,
						}),
					).catch((e) => console.warn("addIceCandidate failed:", e));
				}
			}
		},
		// handleOffer is stable (defined below)
		[],
	);

	const handleOffer = useCallback(
		async (sidecarId: string, sdp: string) => {
			// Close existing connection if any.
			pcRef.current?.close();

			sidecarIdRef.current = sidecarId;
			const pc = new RTCPeerConnection({
				iceServers: [{ urls: "stun:stun.l.google.com:19302" }],
			});
			pcRef.current = pc;

			// Handle ICE candidates — send to sidecar via WS signaling.
			pc.onicecandidate = (event) => {
				if (event.candidate) {
					optionsRef.current.sendWs({
						type: "RtcIceCandidate",
						sidecar_id: sidecarId,
						candidate: event.candidate.candidate,
						sdp_mid: event.candidate.sdpMid,
						sdp_mline_index: event.candidate.sdpMLineIndex,
					});
				}
			};

			// Handle incoming audio track.
			pc.ontrack = (event) => {
				if (event.track.kind === "audio") {
					const audio = new Audio();
					audio.srcObject = new MediaStream([event.track]);
					audio.play().catch(() => {
						// Autoplay blocked — will play on user interaction.
					});
				}
			};

			// Handle data channel from sidecar.
			pc.ondatachannel = (event) => {
				const dc = event.channel;
				dc.binaryType = "arraybuffer";
				dcRef.current = dc;

				dc.onopen = () => {
					console.log("WebRTC data channel open");
					setConnected(true);
				};
				dc.onclose = () => {
					console.log("WebRTC data channel closed");
					setConnected(false);
				};
				dc.onmessage = (msgEvent) => {
					try {
						const msg = decode(
							new Uint8Array(msgEvent.data),
						) as DcServerMsg;
						handleDcMessage(sidecarId, msg);
					} catch (e) {
						console.warn("Failed to decode DC message:", e);
					}
				};
			};

			// Set remote description (the offer from sidecar).
			await pc.setRemoteDescription(
				new RTCSessionDescription({ type: "offer", sdp }),
			);

			// Create and send answer.
			const answer = await pc.createAnswer();
			await pc.setLocalDescription(answer);

			optionsRef.current.sendWs({
				type: "RtcAnswer",
				sidecar_id: sidecarId,
				sdp: answer.sdp ?? "",
			});
		},
		[],
	);

	const handleDcMessage = useCallback(
		(sidecarId: string, msg: DcServerMsg) => {
			const opts = optionsRef.current;
			switch (msg.t) {
				case "d":
					opts.onDisplayUpdate?.(sidecarId, msg.c, msg.u);
					break;
				case "pc":
					opts.onProcessConnected?.(
						msg.pid,
						msg.client_id,
						msg.command,
					);
					break;
				case "pe":
					opts.onProcessExited?.(msg.pid, msg.exit_code);
					break;
				case "id":
					console.warn(
						`Input dropped for ${msg.window_id}: ${msg.reason}`,
					);
					break;
				case "cb":
					// Clipboard data — could be handled here.
					break;
				case "co":
					// Clipboard offer — could be handled here.
					break;
			}
		},
		[],
	);

	const connectToSidecar = useCallback(
		(sidecarId: string) => {
			sidecarIdRef.current = sidecarId;
			options.sendWs({ type: "RtcConnect", sidecar_id: sidecarId });
		},
		[options.sendWs],
	);

	return {
		connected,
		handleSignaling,
		connect: connectToSidecar,
		sendInput: useCallback(
			(windowId: string, event: InputEvent) => {
				sendDc({ t: "i", w: windowId, e: event });
			},
			[sendDc],
		),
		sendResizeWindow: useCallback(
			(windowId: string, width: number, height: number) => {
				sendDc({
					t: "rw",
					window_id: windowId,
					width,
					height,
				});
			},
			[sendDc],
		),
		sendResizeScreen: useCallback(
			(width: number, height: number) => {
				sendDc({ t: "rs", width, height });
			},
			[sendDc],
		),
		sendRedraw: useCallback(
			(windowId: string) => {
				sendDc({ t: "r", window_id: windowId });
			},
			[sendDc],
		),
		sendSpawn: useCallback(
			(requestId: string, command: string, args: string[]) => {
				sendDc({
					t: "sp",
					request_id: requestId,
					command,
					args,
				});
			},
			[sendDc],
		),
		sendKill: useCallback(
			(requestId: string, pid: number) => {
				sendDc({ t: "kp", request_id: requestId, pid });
			},
			[sendDc],
		),
	};
}
