import { useState } from "react";
import s from "./Dock.module.css";
import type { SidecarInfo } from "./types";

interface DockProps {
	connected: boolean;
	sidecars: SidecarInfo[];
	onSpawn: (sidecarId: string, command: string, args: string[]) => void;
}

export function Dock({ connected, sidecars, onSpawn }: DockProps) {
	const [command, setCommand] = useState("xeyes");
	const [args, setArgs] = useState("");

	const sidecar = sidecars[0]; // For now, use first connected sidecar

	return (
		<div className={s.dock} data-testid="dock">
			<div className={s.sidecar}>
				<span
					className={`${s.dot} ${connected && sidecar ? s.online : s.offline}`}
				/>
				<span>{sidecar?.name ?? "No sidecar"}</span>
			</div>
			<input
				type="text"
				value={command}
				onChange={(e) => setCommand(e.target.value)}
				placeholder="command"
				className={s.spawnInput}
			/>
			<input
				type="text"
				value={args}
				onChange={(e) => setArgs(e.target.value)}
				placeholder="args"
				className={s.spawnInput}
			/>
			<button
				type="button"
				className={s.spawnButton}
				disabled={!sidecar}
				onClick={() => {
					if (sidecar) {
						onSpawn(sidecar.id, command, args ? args.split(" ") : []);
					}
				}}
				data-testid="spawn-button"
			>
				Spawn
			</button>
		</div>
	);
}
