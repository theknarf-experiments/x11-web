import { type RefObject, useEffect, useRef, useState } from "react";
import { ClientRenderer } from "./ClientRenderer";
import { decodeFrame } from "./rtcWire";

/**
 * Routes inbound `Frame` payloads from the WebRTC media
 * DataChannel to the per-window `ClientRenderer` map (for
 * `PutImage`) or the thumbnails Map (for `WindowThumbnail`).
 *
 * `renderersRef` is exposed because two other call sites in the
 * app already need direct access — the OCIF node sync effect
 * evicts entries for windows that aren't on the canvas, and the
 * JSX render path lazily creates new renderers when a new window
 * appears. Owning the routing-side writes here while still
 * exposing the ref is the smallest change that preserves the
 * existing access pattern.
 */
type DataChannelHandler = ((data: Uint8Array) => void) | null;

interface UseFrameRouterArgs {
	onDataChannelMessage: (cb: DataChannelHandler) => void;
}

interface UseFrameRouterResult {
	/** Per-window renderer map. Caller may also evict / lazy-init
	 *  via the same ref. */
	renderersRef: RefObject<Map<string, ClientRenderer>>;
	/** windowId → object-URL for the latest thumbnail. */
	thumbnails: Map<string, string>;
}

export function useFrameRouter({
	onDataChannelMessage,
}: UseFrameRouterArgs): UseFrameRouterResult {
	const renderersRef = useRef<Map<string, ClientRenderer>>(new Map());
	const [thumbnails, setThumbnails] = useState<Map<string, string>>(
		() => new Map(),
	);

	useEffect(() => {
		onDataChannelMessage((bytes) => {
			const msg = decodeFrame(bytes);
			if (!msg) return;
			if (msg.kind === "putImage") {
				const renderers = renderersRef.current;
				let r = renderers.get(msg.windowId);
				if (!r) {
					r = new ClientRenderer(msg.width || 1, msg.height || 1);
					renderers.set(msg.windowId, r);
				}
				r.pushPutImage(msg.x, msg.y, msg.data);
				return;
			}
			if (msg.kind === "thumbnail") {
				// Copy the bytes — the decoder hands back a view that
				// may share storage with the reassembler buffer, which
				// gets reused on the next frame.
				const copy = new Uint8Array(msg.data);
				const url = URL.createObjectURL(
					new Blob([copy], { type: "image/webp" }),
				);
				setThumbnails((prev) => {
					const next = new Map(prev);
					const old = next.get(msg.windowId);
					if (old) URL.revokeObjectURL(old);
					next.set(msg.windowId, url);
					return next;
				});
			}
		});
		return () => onDataChannelMessage(null);
	}, [onDataChannelMessage]);

	return { renderersRef, thumbnails };
}
