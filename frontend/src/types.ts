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
	  };

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
	  }
	| { kind: "WindowDestroyed"; window_id: string }
	| { kind: "WindowMapped"; window_id: string; is_top_level?: boolean }
	| { kind: "WindowUnmapped"; window_id: string }
	| {
			kind: "WindowConfigured";
			window_id: string;
			x: number;
			y: number;
			width: number;
			height: number;
	  }
	| {
			kind: "FillRect";
			window_id: string;
			x: number;
			y: number;
			width: number;
			height: number;
			color: number;
	  }
	| {
			kind: "DrawLines";
			window_id: string;
			points: [number, number][];
			color: number;
			line_width: number;
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
			kind: "CopyArea";
			src_window_id: string;
			dst_window_id: string;
			src_x: number;
			src_y: number;
			dst_x: number;
			dst_y: number;
			width: number;
			height: number;
	  }
	| {
			kind: "ClearArea";
			window_id: string;
			x: number;
			y: number;
			width: number;
			height: number;
	  }
	| {
			kind: "DrawArc";
			window_id: string;
			x: number;
			y: number;
			width: number;
			height: number;
			angle1: number;
			angle2: number;
			filled: boolean;
			color: number;
	  }
	| {
			kind: "CursorChanged";
			window_id: string;
			cursor: string;
	  }
	| { kind: "WindowFocused"; window_id: string | null };

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
	| { kind: "MotionNotify"; x: number; y: number; state: number };
