import { useState } from "react";
import s from "./Dock.module.css";
import type { SidecarInfo } from "./types";

interface DockWindow {
	clientId: string;
	title: string;
	color: string;
}

interface DockProps {
	connected: boolean;
	sidecars: SidecarInfo[];
	windows: DockWindow[];
	onSpawn: (sidecarId: string, command: string, args: string[]) => void;
	onFocusWindow: (clientId: string) => void;
}

export function Dock({
	connected,
	sidecars,
	windows,
	onSpawn,
	onFocusWindow,
}: DockProps) {
	const [showSpawn, setShowSpawn] = useState(false);
	const [command, setCommand] = useState("xeyes");
	const [args, setArgs] = useState("");

	const sidecar = sidecars[0];

	function handleSpawn() {
		if (!sidecar) return;
		onSpawn(sidecar.id, command, args ? args.split(" ") : []);
		setShowSpawn(false);
	}

	return (
		<div className={s.dock} data-testid="dock">
			{/* App icons for running windows */}
			{windows.map((win) => (
				<button
					key={win.clientId}
					type="button"
					className={s.iconButton}
					style={{ background: win.color }}
					onClick={() => onFocusWindow(win.clientId)}
				>
					<span className={s.tooltip}>{win.title}</span>
					<span className={s.runningDot} />
					{/* First letter as icon */}
					{win.title.charAt(0).toUpperCase()}
				</button>
			))}

			{/* Add button */}
			<div style={{ position: "relative" }}>
				<button
					type="button"
					className={`${s.iconButton} ${s.addButton}`}
					onClick={() => setShowSpawn(!showSpawn)}
					data-testid="spawn-button"
				>
					<span className={s.statusDot} data-testid="connection-status">
						<span
							className={`${s.statusDot} ${connected && sidecar ? s.online : s.offline}`}
						/>
					</span>
					+
				</button>

				{showSpawn && (
					<div className={s.popover}>
						<div className={s.popoverRow}>
							<input
								type="text"
								value={command}
								onChange={(e) => setCommand(e.target.value)}
								placeholder="command"
								className={s.popoverInput}
								onKeyDown={(e) => {
									if (e.key === "Enter") handleSpawn();
								}}
							/>
						</div>
						<div className={s.popoverRow}>
							<input
								type="text"
								value={args}
								onChange={(e) => setArgs(e.target.value)}
								placeholder="args"
								className={s.popoverInput}
								onKeyDown={(e) => {
									if (e.key === "Enter") handleSpawn();
								}}
							/>
						</div>
						<div className={s.popoverRow}>
							<button
								type="button"
								className={s.popoverButton}
								disabled={!sidecar}
								onClick={handleSpawn}
							>
								Spawn
							</button>
						</div>
					</div>
				)}
			</div>
		</div>
	);
}
