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
	  }
	| {
			type: "DisplayUpdate";
			sidecar_id: string;
			client_id: string;
			update: DisplayUpdate;
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
			client_id: string;
			event: InputEvent;
	  };

export type DisplayUpdate =
	| {
			kind: "WindowCreated";
			window_id: number;
			x: number;
			y: number;
			width: number;
			height: number;
	  }
	| { kind: "WindowDestroyed"; window_id: number }
	| { kind: "WindowMapped"; window_id: number }
	| { kind: "WindowUnmapped"; window_id: number }
	| {
			kind: "WindowConfigured";
			window_id: number;
			x: number;
			y: number;
			width: number;
			height: number;
	  }
	| {
			kind: "FillRect";
			window_id: number;
			x: number;
			y: number;
			width: number;
			height: number;
			color: number;
	  }
	| {
			kind: "DrawLines";
			window_id: number;
			points: [number, number][];
			color: number;
			line_width: number;
	  }
	| {
			kind: "PutImage";
			window_id: number;
			x: number;
			y: number;
			width: number;
			height: number;
			data: number[];
	  }
	| {
			kind: "CopyArea";
			src_window_id: number;
			dst_window_id: number;
			src_x: number;
			src_y: number;
			dst_x: number;
			dst_y: number;
			width: number;
			height: number;
	  }
	| {
			kind: "ClearArea";
			window_id: number;
			x: number;
			y: number;
			width: number;
			height: number;
	  }
	| {
			kind: "DrawArc";
			window_id: number;
			x: number;
			y: number;
			width: number;
			height: number;
			angle1: number;
			angle2: number;
			filled: boolean;
			color: number;
	  };

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
