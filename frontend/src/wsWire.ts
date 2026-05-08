// Cap'n Proto wire format codec for the WebSocket between the SPA
// and the backend. Schema lives in `crates/ws-wire/schema/ws.capnp`;
// `pnpm codegen` regenerates `frontend/src/generated/ws.ts`.
//
// This module wraps the generated readers/writers in the same TS
// types `useBackendSocket` already uses (`FrontendToBackend` /
// `BackendToFrontend` from `./types`), so the SPA call sites need
// no churn — only the WS transport flips from JSON.parse / stringify
// to encode / decode here.
//
// Mirrors the Rust bridge in `crates/ws-wire/src/bridge.rs`.

import { Message } from "capnp-es";

import {
	BackendMsg,
	type BackendMsg_Payload,
	CheckState,
	type DndEvent as DndEventCapnp,
	FrontendMsg,
	type FrontendMsg_Payload,
	GesturePhase,
	type InputEvent as InputEventCapnp,
	type MenuAction as MenuActionCapnp,
	type MenuActionTarget,
	type MenuItem as MenuItemCapnp,
	MenuItemKind as MenuItemKindCapnp,
	type WindowDescriptor as WindowDescriptorCapnp,
	type WindowUpdate as WindowUpdateCapnp,
	WindowWmState as WindowWmStateCapnp,
} from "./generated/ws";
import type {
	BackendToFrontend,
	DndEventKind,
	FrontendToBackend,
	InputEvent,
	MenuAction,
	MenuItem,
	MenuItemKind,
	ProcessInfo,
	SidecarInfo,
	WindowDescriptor,
	WindowUpdate,
	WindowWmState,
} from "./types";

// ============================================================
// Frontend → Backend (encode)
// ============================================================

export function encodeFrontendMsg(
	msg: FrontendToBackend,
	traceparent: string,
): Uint8Array {
	const root = new Message();
	const f = root.initRoot(FrontendMsg);
	f.traceparent = traceparent;
	writeFrontendPayload(f._initPayload(), msg);
	return new Uint8Array(root.toArrayBuffer());
}

function writeFrontendPayload(
	p: FrontendMsg_Payload,
	msg: FrontendToBackend,
): void {
	switch (msg.type) {
		case "OpenWorkspace": {
			const ow = p._initOpenWorkspace();
			if (msg.id !== null) ow.id = msg.id;
			return;
		}
		case "SpawnProcess": {
			const sp = p._initSpawnProcess();
			sp.requestId = msg.request_id;
			sp.sidecarId = msg.sidecar_id;
			sp.workspaceId = msg.workspace_id;
			sp.command = msg.command;
			const args = sp._initArgs(msg.args.length);
			for (let i = 0; i < msg.args.length; i++) args.set(i, msg.args[i]);
			return;
		}
		case "KillProcess": {
			const kp = p._initKillProcess();
			kp.requestId = msg.request_id;
			kp.sidecarId = msg.sidecar_id;
			kp.pid = msg.pid;
			return;
		}
		case "InputEvent": {
			const ie = p._initInputEvent();
			ie.sidecarId = msg.sidecar_id;
			ie.windowId = msg.window_id;
			writeInputEvent(ie._initEvent(), msg.event);
			return;
		}
		case "ResizeWindow": {
			const r = p._initResizeWindow();
			r.sidecarId = msg.sidecar_id;
			r.windowId = msg.window_id;
			r.width = msg.width;
			r.height = msg.height;
			return;
		}
		case "RtcOffer": {
			p._initRtcOffer().sdp = msg.sdp;
			return;
		}
		case "RtcIceCandidate": {
			const c = p._initRtcIceCandidate();
			c.candidate = msg.candidate;
			if (msg.sdp_mid != null) c.sdpMid = msg.sdp_mid;
			if (msg.sdp_mline_index != null) {
				c.sdpMlineIndexHas = true;
				c.sdpMlineIndex = msg.sdp_mline_index;
			}
			return;
		}
	}
}

// ============================================================
// Backend → Frontend (decode)
// ============================================================

export interface DecodedBackendMsg {
	msg: BackendToFrontend;
	traceparent: string;
}

/** Returns `null` on a malformed frame or the forward-compat
 *  `noVariant` fallback (a future schema addition the current
 *  client doesn't know about). */
export function decodeBackendMsg(buf: Uint8Array): DecodedBackendMsg | null {
	try {
		const copy = new Uint8Array(buf.byteLength);
		copy.set(buf);
		const root = new Message(copy.buffer, /* packed */ false).getRoot(
			BackendMsg,
		);
		const traceparent = root.traceparent;
		const msg = readBackendPayload(root.payload);
		if (msg === null) return null;
		return { msg, traceparent };
	} catch {
		return null;
	}
}

function readBackendPayload(p: BackendMsg_Payload): BackendToFrontend | null {
	if (p._isSidecarList) {
		const sl = p.sidecarList;
		const sidecars: SidecarInfo[] = [];
		for (let i = 0; i < sl.sidecars.length; i++) {
			const s = sl.sidecars.get(i);
			sidecars.push({ id: s.id, name: s.name });
		}
		return { type: "SidecarList", sidecars };
	}
	if (p._isWorkspace) {
		const w = p.workspace.workspace;
		return { type: "Workspace", workspace: { id: w.id, name: w.name } };
	}
	if (p._isCommandResult) {
		const c = p.commandResult;
		return {
			type: "CommandResult",
			request_id: c.requestId,
			success: c.success,
			message: c.message,
		};
	}
	if (p._isProcessList) {
		const pl = p.processList;
		const processes: ProcessInfo[] = [];
		for (let i = 0; i < pl.processes.length; i++) {
			const e = pl.processes.get(i);
			processes.push({
				pid: e.pid,
				client_id: e.clientId,
				command: e.command,
			});
		}
		return { type: "ProcessList", sidecar_id: pl.sidecarId, processes };
	}
	if (p._isWindowUpdate) {
		const update = readWindowUpdate(p.windowUpdate.update);
		if (update === null) return null;
		return { type: "WindowUpdate", update };
	}
	if (p._isWindowList) {
		const wl = p.windowList;
		const windows: WindowDescriptor[] = [];
		for (let i = 0; i < wl.windows.length; i++) {
			windows.push(readWindowDescriptor(wl.windows.get(i)));
		}
		return { type: "WindowList", windows };
	}
	if (p._isBell) {
		return { type: "Bell", percent: p.bell.percent };
	}
	if (p._isRtcAnswer) {
		return { type: "RtcAnswer", sdp: p.rtcAnswer.sdp };
	}
	if (p._isRtcIceCandidate) {
		const c = p.rtcIceCandidate;
		return {
			type: "RtcIceCandidate",
			candidate: c.candidate,
			// Capnp-es doesn't generate `_has*()` for Text fields, so
			// optional strings round-trip via empty-as-sentinel. Real
			// menu labels / sdp mids are never the empty string.
			sdp_mid: c.sdpMid !== "" ? c.sdpMid : null,
			sdp_mline_index: c.sdpMlineIndexHas ? c.sdpMlineIndex : null,
		};
	}
	return null;
}

function readWindowUpdate(u: WindowUpdateCapnp): WindowUpdate | null {
	if (u._isTitleChanged) {
		const t = u.titleChanged;
		return { kind: "TitleChanged", window_id: t.windowId, title: t.title };
	}
	if (u._isStateChanged) {
		const s = u.stateChanged;
		return {
			kind: "StateChanged",
			window_id: s.windowId,
			state: readWmState(s.state),
		};
	}
	if (u._isFocused) {
		const f = u.focused;
		return {
			kind: "Focused",
			window_id: f.windowId !== "" ? f.windowId : null,
		};
	}
	if (u._isMenuStructure) {
		const m = u.menuStructure;
		const menu: MenuItem[] = [];
		for (let i = 0; i < m.items.length; i++) {
			menu.push(readMenuItem(m.items.get(i)));
		}
		return { kind: "MenuStructure", window_id: m.windowId, menu };
	}
	return null;
}

function readWindowDescriptor(w: WindowDescriptorCapnp): WindowDescriptor {
	return {
		window_id: w.windowId,
		sidecar_id: w.sidecarId,
		pid: w.pid,
		command: w.command,
		x: w.x,
		y: w.y,
		width: w.width,
		height: w.height,
		border_width: w.borderWidth,
		border_pixel: w.borderPixel,
		override_redirect: w.overrideRedirect,
		resizable: w.resizable,
	};
}

// ============================================================
// Shared helpers
// ============================================================

function readWmState(s: WindowWmStateCapnp): WindowWmState {
	switch (s) {
		case WindowWmStateCapnp.NORMAL:
			return "normal";
		case WindowWmStateCapnp.MINIMIZED:
			return "minimized";
		case WindowWmStateCapnp.MAXIMIZED:
			return "maximized";
		case WindowWmStateCapnp.FULLSCREEN:
			return "fullscreen";
		case WindowWmStateCapnp.CLOSE:
			return "close";
	}
}

function writeWmState(s: WindowWmState): WindowWmStateCapnp {
	switch (s) {
		case "normal":
			return WindowWmStateCapnp.NORMAL;
		case "minimized":
			return WindowWmStateCapnp.MINIMIZED;
		case "maximized":
			return WindowWmStateCapnp.MAXIMIZED;
		case "fullscreen":
			return WindowWmStateCapnp.FULLSCREEN;
		case "close":
			return WindowWmStateCapnp.CLOSE;
	}
}

function readMenuItem(item: MenuItemCapnp): MenuItem {
	const checked =
		item.checked === CheckState.NOT_APPLICABLE
			? undefined
			: item.checked === CheckState.CHECKED;
	const children: MenuItem[] = [];
	for (let i = 0; i < item.children.length; i++) {
		children.push(readMenuItem(item.children.get(i)));
	}
	return {
		id: item.id,
		label: item.label !== "" ? item.label : undefined,
		kind: readMenuItemKind(item.kind),
		enabled: item.enabled,
		visible: item.visible,
		checked,
		accelerator: item.accelerator !== "" ? item.accelerator : undefined,
		icon: item.icon !== "" ? item.icon : undefined,
		action: item._hasAction() ? readMenuAction(item.action) : undefined,
		children,
	};
}

function readMenuItemKind(k: MenuItemKindCapnp): MenuItemKind {
	switch (k) {
		case MenuItemKindCapnp.NORMAL:
			return "normal";
		case MenuItemKindCapnp.SUBMENU:
			return "submenu";
		case MenuItemKindCapnp.SEPARATOR:
			return "separator";
		case MenuItemKindCapnp.CHECKBOX:
			return "checkbox";
		case MenuItemKindCapnp.RADIO:
			return "radio";
	}
}

// Round-trippable target shape. The TS-side `MenuAction.target` is
// declared as `unknown` (the SPA never inspects it — just hands it
// back on activation), so the bridge round-trips through this
// tagged union internally and casts at the API boundary.
type BridgeTarget =
	| { kind: "String"; value: string }
	| { kind: "Bool"; value: boolean }
	| { kind: "Int32"; value: number }
	| { kind: "UInt32"; value: number }
	| { kind: "Int64"; value: number }
	| { kind: "Float64"; value: number };

function readMenuAction(a: MenuActionCapnp): MenuAction {
	const target = a._hasTarget() ? readMenuActionTarget(a.target) : undefined;
	return { name: a.name, target };
}

function writeMenuAction(b: MenuActionCapnp, a: MenuAction): void {
	b.name = a.name;
	if (a.target !== null && a.target !== undefined) {
		writeMenuActionTarget(b._initTarget(), a.target as BridgeTarget);
	}
}

function readMenuActionTarget(t: MenuActionTarget): BridgeTarget {
	if (t._isString) return { kind: "String", value: t.string };
	if (t._isBoolean) return { kind: "Bool", value: t.boolean };
	if (t._isInt32) return { kind: "Int32", value: t.int32 };
	if (t._isUInt32) return { kind: "UInt32", value: t.uInt32 };
	if (t._isInt64) return { kind: "Int64", value: Number(t.int64) };
	if (t._isFloat64) return { kind: "Float64", value: t.float64 };
	// Should be unreachable; pick a safe fallback.
	return { kind: "String", value: "" };
}

function writeMenuActionTarget(b: MenuActionTarget, t: BridgeTarget): void {
	switch (t.kind) {
		case "String":
			b.string = t.value;
			return;
		case "Bool":
			b.boolean = t.value;
			return;
		case "Int32":
			b.int32 = t.value;
			return;
		case "UInt32":
			b.uInt32 = t.value;
			return;
		case "Int64":
			b.int64 = BigInt(t.value);
			return;
		case "Float64":
			b.float64 = t.value;
			return;
	}
}

// ---- InputEvent ----

function writeInputEvent(b: InputEventCapnp, ev: InputEvent): void {
	const p = b._initPayload();
	switch (ev.kind) {
		case "KeyPress": {
			const k = p._initKeyPress();
			k.keycode = ev.keycode;
			k.state = ev.state;
			return;
		}
		case "KeyRelease": {
			const k = p._initKeyRelease();
			k.keycode = ev.keycode;
			k.state = ev.state;
			return;
		}
		case "ButtonPress": {
			const x = p._initButtonPress();
			x.button = ev.button;
			x.x = ev.x;
			x.y = ev.y;
			x.state = ev.state;
			return;
		}
		case "ButtonRelease": {
			const x = p._initButtonRelease();
			x.button = ev.button;
			x.x = ev.x;
			x.y = ev.y;
			x.state = ev.state;
			return;
		}
		case "MotionNotify": {
			const m = p._initMotionNotify();
			m.x = ev.x;
			m.y = ev.y;
			m.state = ev.state;
			return;
		}
		case "MenuActivate": {
			writeMenuAction(p._initMenuActivate()._initAction(), ev.action);
			return;
		}
		case "WindowManage": {
			p._initWindowManage().action = writeWmState(ev.action);
			return;
		}
		case "DndBridge": {
			writeDndEvent(p._initDndBridge()._initEvent(), ev.event);
			return;
		}
		case "TouchBegin": {
			const t = p._initTouchBegin();
			t.touchId = ev.touch_id;
			t.x = ev.x;
			t.y = ev.y;
			t.state = ev.state;
			return;
		}
		case "TouchUpdate": {
			const t = p._initTouchUpdate();
			t.touchId = ev.touch_id;
			t.x = ev.x;
			t.y = ev.y;
			t.state = ev.state;
			return;
		}
		case "TouchEnd": {
			const t = p._initTouchEnd();
			t.touchId = ev.touch_id;
			t.x = ev.x;
			t.y = ev.y;
			t.state = ev.state;
			return;
		}
		case "GestureSwipe": {
			const g = p._initGestureSwipe();
			g.phase = writeGesturePhase(ev.phase);
			g.fingers = ev.fingers;
			g.dx = ev.dx;
			g.dy = ev.dy;
			return;
		}
		case "GesturePinch": {
			const g = p._initGesturePinch();
			g.phase = writeGesturePhase(ev.phase);
			g.fingers = ev.fingers;
			g.dx = ev.dx;
			g.dy = ev.dy;
			g.scale = ev.scale;
			g.rotation = ev.rotation;
			return;
		}
		case "CompositionEvent": {
			const c = p._initCompositionEvent();
			c.phase = ev.phase;
			c.text = ev.text;
			return;
		}
	}
}

type GesturePhaseStr = "Begin" | "Update" | "End";

function writeGesturePhase(g: GesturePhaseStr): GesturePhase {
	switch (g) {
		case "Begin":
			return GesturePhase.BEGIN;
		case "Update":
			return GesturePhase.UPDATE;
		case "End":
			return GesturePhase.END;
	}
}

function writeDndEvent(b: DndEventCapnp, ev: DndEventKind): void {
	const p = b._initPayload();
	switch (ev.kind) {
		case "Enter": {
			const list = p._initEnter()._initMimeTypes(ev.mime_types.length);
			for (let i = 0; i < ev.mime_types.length; i++)
				list.set(i, ev.mime_types[i]);
			return;
		}
		case "Position": {
			const pos = p._initPosition();
			pos.x = ev.x;
			pos.y = ev.y;
			return;
		}
		case "Drop": {
			const d = p._initDrop();
			d.mimeType = ev.mime_type;
			// ev.data is a string ("base64-encoded") in TS — we ship
			// raw bytes on the wire, so decode here. This mirrors the
			// pre-existing behaviour: types.ts declared the field as
			// a base64 string for JSON survival, capnp has native
			// `Data` so the conversion happens in the bridge.
			const bytes = base64ToBytes(ev.data);
			const out = d._initData(bytes.length);
			out.copyBuffer(bytes);
			return;
		}
		case "Leave": {
			p.leave = true;
			return;
		}
	}
}

function base64ToBytes(s: string): Uint8Array {
	const bin = atob(s);
	const out = new Uint8Array(bin.length);
	for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
	return out;
}
