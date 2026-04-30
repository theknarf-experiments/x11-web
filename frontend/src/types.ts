// Protocol types matching crates/protocol/src/lib.rs

export interface SidecarInfo {
	id: string;
	name: string;
}

export interface ProcessInfo {
	pid: number;
	command: string;
}

// Backend -> Frontend messages
export type BackendToFrontend =
	| { type: "SidecarList"; sidecars: SidecarInfo[] }
	| { type: "SidecarConnected"; sidecar: SidecarInfo }
	| { type: "SidecarDisconnected"; sidecar_id: string }
	| {
			type: "CommandResult";
			request_id: string;
			success: boolean;
			message: string;
	  }
	| { type: "ProcessList"; sidecar_id: string; processes: ProcessInfo[] }
	| {
			type: "ProcessExited";
			sidecar_id: string;
			pid: number;
			exit_code: number | null;
	  }
	| {
			type: "ProcessConnected";
			sidecar_id: string;
			pid: number;
			client_id: string;
			command: string;
	  }
	| {
			type: "DisplayUpdate";
			sidecar_id: string;
			client_id: string;
			update: DisplayUpdate;
	  }
	| {
			type: "ConnectedProcessesList";
			processes: {
				sidecar_id: string;
				pid: number;
				client_id: string;
				command: string;
			}[];
	  }
	| {
			type: "WindowStateList";
			windows: {
				client_id: string;
				sidecar_id: string;
				pid: number;
				x: number;
				y: number;
				color: string;
			}[];
	  }
	| {
			type: "WindowStateChanged";
			client_id: string;
			x: number;
			y: number;
			color: string;
	  }
	| {
			type: "InputDropped";
			sidecar_id: string;
			window_id: string;
			reason: string;
	  }
	| {
			type: "ClipboardData";
			sidecar_id: string;
			selection: string;
			mime_type: string;
			data: string;
	  }
	| {
			type: "ClipboardOffer";
			sidecar_id: string;
			selection: string;
			mime_types: string[];
	  }
	| {
			type: "RtcOffer";
			sidecar_id: string;
			sdp: string;
	  }
	| {
			type: "RtcIceCandidate";
			sidecar_id: string;
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
	| { type: "ListProcesses"; request_id: string; sidecar_id: string }
	| { type: "SubscribeDisplay"; sidecar_id: string }
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
			type: "UpdateWindowState";
			client_id: string;
			sidecar_id: string;
			x: number;
			y: number;
			color: string;
	  }
	| {
			type: "RequestClipboard";
			sidecar_id: string;
			selection: string;
			mime_type: string;
	  }
	| {
			type: "SetClipboard";
			sidecar_id: string;
			selection: string;
			mime_type: string;
			data: string;
	  }
	| {
			type: "ResizeScreen";
			sidecar_id: string;
			width: number;
			height: number;
	  }
	| {
			type: "RtcConnect";
			sidecar_id: string;
	  }
	| {
			type: "RtcAnswer";
			sidecar_id: string;
			sdp: string;
	  }
	| {
			type: "RtcIceCandidate";
			sidecar_id: string;
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

export type DisplayUpdate =
	| { kind: "TitleChanged"; window_id: string; title: string }
	| {
			kind: "WindowCreated";
			window_id: string;
			x: number;
			y: number;
			width: number;
			height: number;
			is_top_level?: boolean;
			override_redirect?: boolean;
			border_width?: number;
			border_pixel?: number;
	  }
	| { kind: "WindowDestroyed"; window_id: string }
	| { kind: "WindowMapped"; window_id: string; is_top_level?: boolean; override_redirect?: boolean }
	| { kind: "WindowUnmapped"; window_id: string }
	| {
			kind: "WindowConfigured";
			window_id: string;
			x: number;
			y: number;
			width: number;
			height: number;
			border_width?: number;
			border_pixel?: number;
	  }
	| {
			kind: "PutImage";
			window_id: string;
			x: number;
			y: number;
			width: number;
			height: number;
			data: string;
	  }
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
			kind: "WindowStateChanged";
			window_id: string;
			state: WindowWmState;
	  }
	| { kind: "WindowFocused"; window_id: string | null }
	| {
			kind: "MenuStructure";
			window_id: string;
			menu: MenuItem[];
	  }
	| { kind: "WindowRaised"; window_id: string }
	| { kind: "WindowUrgent"; window_id: string; urgent: boolean }
	| {
			kind: "WindowIconChanged";
			window_id: string;
			width: number;
			height: number;
			data: string;
	  }
	| { kind: "Bell"; percent: number };

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
