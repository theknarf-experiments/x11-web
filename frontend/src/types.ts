// Protocol types matching crates/protocol/src/lib.rs

export interface SidecarInfo {
	id: string;
	name: string;
}

export interface Workspace {
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
	| { type: "Workspace"; workspace: Workspace }
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
	| { type: "OpenWorkspace"; id: string | null }
	| {
			type: "SpawnProcess";
			request_id: string;
			sidecar_id: string;
			workspace_id: string;
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
	| { type: "RtcOffer"; sdp: string }
	| {
			type: "RtcIceCandidate";
			candidate: string;
			sdp_mid: string | null;
			sdp_mline_index: number | null;
	  };

/** Window WM states. */
export type WindowWmState =
	| "normal"
	| "minimized"
	| "maximized"
	| "fullscreen"
	| "close";

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
	/** Whether the user can drag-resize this window. Set by the
	 * sidecar — X11 sidecars always report true; the macOS sidecar
	 * probes AX (`AXSize` settable) so fixed-size apps like
	 * Calculator's Basic mode report false and the frontend hides
	 * its resize handles. */
	resizable: boolean;
}

export type WindowUpdate =
	| { kind: "TitleChanged"; window_id: string; title: string }
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
	  };

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
	| {
			kind: "TouchBegin";
			touch_id: number;
			x: number;
			y: number;
			state: number;
	  }
	| {
			kind: "TouchUpdate";
			touch_id: number;
			x: number;
			y: number;
			state: number;
	  }
	| { kind: "TouchEnd"; touch_id: number; x: number; y: number; state: number }
	| {
			kind: "GestureSwipe";
			dx: number;
			dy: number;
			fingers: number;
			phase: "Begin" | "Update" | "End";
	  }
	| {
			kind: "GesturePinch";
			dx: number;
			dy: number;
			scale: number;
			rotation: number;
			fingers: number;
			phase: "Begin" | "Update" | "End";
	  }
	| { kind: "MenuActivate"; action: MenuAction }
	| { kind: "WindowManage"; action: WindowWmState }
	| { kind: "DndBridge"; event: DndEventKind }
	| {
			kind: "CompositionEvent";
			phase: "start" | "update" | "end";
			text: string;
	  };
