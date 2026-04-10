import { useEffect, useRef, useState, ViewTransition } from "react";
import { AppContextMenu, getAppContextMenuItems } from "./AppContextMenu";
import s from "./Dock.module.css";
import type { SidecarInfo } from "./types";

export interface DockProcess {
	sidecarId: string;
	pid: number;
	title: string;
	color: string;
}

interface DockProps {
	connected: boolean;
	sidecars: SidecarInfo[];
	processes: DockProcess[];
	onSpawn: (sidecarId: string, command: string, args: string[]) => void;
	onClose: (sidecarId: string, pid: number) => void;
	onFocusWindow: (sidecarId: string, pid: number) => void;
}

export function Dock({
	connected,
	sidecars,
	processes,
	onSpawn,
	onClose,
	onFocusWindow,
}: DockProps) {
	const [showSpawn, setShowSpawn] = useState(false);
	const [contextMenu, setContextMenu] = useState<{
		sidecarId: string;
		pid: number;
		x: number;
		y: number;
	} | null>(null);
	const [command, setCommand] = useState("xeyes");
	const [args, setArgs] = useState("");
	const [selectedSidecar, setSelectedSidecar] = useState<string>("");
	const spawnRef = useRef<HTMLDivElement>(null);

	// Auto-select first sidecar
	useEffect(() => {
		if (!selectedSidecar && sidecars.length > 0) {
			setSelectedSidecar(sidecars[0].id);
		}
	}, [sidecars, selectedSidecar]);

	// Close the spawn popover on outside click. The context menu has
	// its own outside-click handler inside `AppContextMenu`.
	useEffect(() => {
		if (!showSpawn) return;
		function handleClick(e: MouseEvent) {
			if (
				spawnRef.current &&
				!spawnRef.current.contains(e.target as Node)
			) {
				setShowSpawn(false);
			}
		}
		document.addEventListener("pointerdown", handleClick);
		return () => document.removeEventListener("pointerdown", handleClick);
	}, [showSpawn]);

	function handleSpawn() {
		if (!selectedSidecar) return;
		onSpawn(selectedSidecar, command, args ? args.split(" ") : []);
		setShowSpawn(false);
	}

	function handleContextMenu(
		e: React.MouseEvent,
		sidecarId: string,
		pid: number,
	) {
		e.preventDefault();
		e.stopPropagation();
		setContextMenu({ sidecarId, pid, x: e.clientX, y: e.clientY });
	}

	return (
		<>
			<div className={s.dock} data-testid="dock">
				{/* App icons — one per process */}
				{processes.map((proc) => (
					<ViewTransition
						key={`${proc.sidecarId}:${proc.pid}`}
						enter="dock-icon-in"
						exit="dock-icon-out"
					>
						<button
							type="button"
							className={s.iconButton}
							style={{ background: proc.color }}
							onClick={() => onFocusWindow(proc.sidecarId, proc.pid)}
							onContextMenu={(e) =>
								handleContextMenu(e, proc.sidecarId, proc.pid)
							}
						>
							<span className={s.tooltip}>{proc.title}</span>
							<span className={s.runningDot} />
							{proc.title.charAt(0).toUpperCase()}
						</button>
					</ViewTransition>
				))}

				{/* Separator between apps and add button */}
				{processes.length > 0 && <div className={s.separator} />}

				{/* Add button */}
				<div ref={spawnRef} style={{ position: "relative" }}>
					<button
						type="button"
						className={`${s.iconButton} ${s.addButton}`}
						onClick={() => {
							setShowSpawn(!showSpawn);
							setContextMenu(null);
						}}
						data-testid="spawn-button"
					>
						<span className={s.statusDot}>
							<span
								className={`${s.statusDotInner} ${connected && sidecars.length > 0 ? s.online : s.offline}`}
								data-testid="connection-status"
							/>
						</span>
						+
					</button>

					{showSpawn && (
						<div className={s.popover}>
							{sidecars.length > 1 && (
								<div className={s.popoverRow}>
									<select
										className={s.popoverSelect}
										value={selectedSidecar}
										onChange={(e) => setSelectedSidecar(e.target.value)}
									>
										{sidecars.map((sc) => (
											<option key={sc.id} value={sc.id}>
												{sc.name}
											</option>
										))}
									</select>
								</div>
							)}
							{sidecars.length === 1 && (
								<div className={s.popoverLabel}>{sidecars[0].name}</div>
							)}
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
									disabled={!selectedSidecar}
									onClick={handleSpawn}
								>
									Spawn
								</button>
							</div>
						</div>
					)}
				</div>
			</div>
			{contextMenu && (
				<AppContextMenu
					items={getAppContextMenuItems(
						contextMenu.sidecarId,
						contextMenu.pid,
						{ onClose },
					)}
					x={contextMenu.x}
					y={contextMenu.y}
					openUpwards
					onClose={() => setContextMenu(null)}
				/>
			)}
		</>
	);
}
