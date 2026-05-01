// Protocol types matching crates/protocol/src/lib.rs

export interface SidecarInfo {
	id: string;
	name: string;
}

export interface ProcessInfo {
	pid: number;
	client_id: string;
	command: string;
}

// Backend -> Frontend messages
export type BackendToFrontend =
	| { type: "SidecarList"; sidecars: SidecarInfo[] }
	| {
			type: "CommandResult";
			request_id: string;
			success: boolean;
			message: string;
	  }
	| { type: "ProcessList"; sidecar_id: string; processes: ProcessInfo[] }
	| { type: "WindowUpdate"; update: WindowUpdate }
	| { type: "WindowList"; windows: WindowDescriptor[] }
	| { type: "Bell"; percent: number }
	| { type: "RtcAnswer"; sdp: string }
	| {
			type: "RtcIceCandidate";
			candidate: string;
			sdp_mid: string | null;
			sdp_mline_index: number | null;
	  };

// Frontend -> Backend messages
export type FrontendToBackend =
	| {
			type: "SpawnProcess";
			request_id: string;
			sidecar_id: string;
			command: string;
			args: string[];
	  }
	| { type: "KillProcess"; request_id: string; sidecar_id: string; pid: number }
	| {
			type: "InputEvent";
			sidecar_id: string;
			window_id: string;
			event: InputEvent;
	  }
	| {
			type: "ResizeWindow";
			sidecar_id: string;
			window_id: string;
			width: number;
			height: number;
	  }
	| {
			type: "UpdateWindowPosition";
			window_id: string;
			x: number;
			y: number;
	  }
	| { type: "RtcOffer"; sdp: string }
	| {
			type: "RtcIceCandidate";
			candidate: string;
			sdp_mid: string | null;
			sdp_mline_index: number | null;
	  };

/** Animated cursor frame. */
export interface AnimCursorFrame {
	/** Base64-encoded ARGB pixel data. */
	pixels: string;
	width: number;
	height: number;
	hotspot_x: number;
	hotspot_y: number;
	delay_ms: number;
}

/** Window WM states. */
export type WindowWmState = "normal" | "minimized" | "maximized" | "fullscreen" | "close";

/** Drag-and-drop event kinds mapped from XdndDrop protocol. */
export type DndEventKind =
	| { kind: "Enter"; mime_types: string[] }
	| { kind: "Position"; x: number; y: number }
	| { kind: "Drop"; mime_type: string; data: string }
	| { kind: "Leave" };

/** Focus policy for the window manager. */
export type FocusPolicy = "click-to-focus" | "focus-follows-mouse";

/** A visible window from the backend's authoritative `WindowList`. */
export interface WindowDescriptor {
	window_id: string;
	sidecar_id: string;
	pid: number;
	command: string;
	x: number;
	y: number;
	width: number;
	height: number;
	border_width: number;
	border_pixel: number;
	override_redirect: boolean;
	/** True when (x, y) is a meaningful position (X11 server position
	 * for popups, or a cross-frontend tracked position for top-level
	 * windows). False when it's the X11 default and the frontend may
	 * apply its own layout heuristic. */
	placed: boolean;
}

export type WindowUpdate =
	| { kind: "TitleChanged"; window_id: string; title: string }
	| {
			kind: "CursorChanged";
			window_id: string;
			cursor: string;
	  }
	| {
			kind: "CursorBitmap";
			window_id: string;
			width: number;
			height: number;
			hotspot_x: number;
			hotspot_y: number;
			data: string;
	  }
	| {
			kind: "CursorAnimated";
			window_id: string;
			frames: AnimCursorFrame[];
	  }
	| {
			kind: "StateChanged";
			window_id: string;
			state: WindowWmState;
	  }
	| { kind: "Focused"; window_id: string | null }
	| {
			kind: "MenuStructure";
			window_id: string;
			menu: MenuItem[];
	  }
	| { kind: "PositionChanged"; window_id: string; x: number; y: number };

export type MenuItemKind =
	| "normal"
	| "submenu"
	| "separator"
	| "checkbox"
	| "radio";

export interface MenuAction {
	name: string;
	target?: unknown;
}

export interface MenuItem {
	id: string;
	label?: string;
	kind: MenuItemKind;
	enabled?: boolean;
	visible?: boolean;
	checked?: boolean;
	accelerator?: string;
	icon?: string;
	action?: MenuAction;
	children?: MenuItem[];
}

export type InputEvent =
	| { kind: "KeyPress"; keycode: number; state: number }
	| { kind: "KeyRelease"; keycode: number; state: number }
	| { kind: "ButtonPress"; button: number; x: number; y: number; state: number }
	| {
			kind: "ButtonRelease";
			button: number;
			x: number;
			y: number;
			state: number;
	  }
	| { kind: "MotionNotify"; x: number; y: number; state: number }
	| { kind: "TouchBegin"; touch_id: number; x: number; y: number; state: number }
	| { kind: "TouchUpdate"; touch_id: number; x: number; y: number; state: number }
	| { kind: "TouchEnd"; touch_id: number; x: number; y: number; state: number }
	| { kind: "GestureSwipe"; dx: number; dy: number; fingers: number; phase: "Begin" | "Update" | "End" }
	| { kind: "GesturePinch"; dx: number; dy: number; scale: number; rotation: number; fingers: number; phase: "Begin" | "Update" | "End" }
	| { kind: "MenuActivate"; action: MenuAction }
	| { kind: "WindowManage"; action: WindowWmState }
	| { kind: "DndBridge"; event: DndEventKind }
	| { kind: "CompositionEvent"; phase: "start" | "update" | "end"; text: string };
