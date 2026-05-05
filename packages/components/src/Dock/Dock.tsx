import { useEffect, useRef, useState, ViewTransition } from "react";
import { Polaroid, PolaroidStack } from "../Polaroid/Polaroid.tsx";
import { Tooltip } from "../Tooltip/Tooltip.tsx";
import s from "./Dock.module.css";

/** A running process represented by an icon in the dock. */
export interface DockProcess {
	sidecarId: string;
	pid: number;
	title: string;
	color: string;
}

/** A connected sidecar — gets a `+` button to spawn into. */
export interface DockSidecar {
	id: string;
	name: string;
}

/** An unattached window (one not yet placed on the canvas) shown
 *  as a draggable thumbnail in the spawn popover. */
export interface DockWindow {
	windowId: string;
	sidecarId: string;
	title: string;
}

/** MIME type used when dragging a window thumbnail out of the
 *  popover. Consumers reading from `dataTransfer` should use the
 *  same constant. */
export const DOCK_WINDOW_DRAG_MIME = "application/x-x11web-window-id";

interface DockProps {
	connected: boolean;
	sidecars: DockSidecar[];
	processes: DockProcess[];
	/** Windows eligible for drag-onto-canvas. Caller filters out
	 *  already-attached windows. */
	windows: DockWindow[];
	/** Live preview blob URLs keyed by `windowId`. */
	thumbnails: Map<string, string>;
	onSpawn: (sidecarId: string, command: string, args: string[]) => void;
	onFocusWindow: (sidecarId: string, pid: number) => void;
	/** Right-click on a process icon — caller renders a context
	 *  menu (e.g. "Close") at the supplied viewport coords. */
	onProcessContextMenu: (
		sidecarId: string,
		pid: number,
		x: number,
		y: number,
	) => void;
}

export function Dock({
	connected,
	sidecars,
	processes,
	windows,
	thumbnails,
	onSpawn,
	onFocusWindow,
	onProcessContextMenu,
}: DockProps) {
	const [openSpawnId, setOpenSpawnId] = useState<string | null>(null);
	const [command, setCommand] = useState("xeyes");
	const [args, setArgs] = useState("");
	const spawnRowRef = useRef<HTMLDivElement>(null);

	// Outside-click closes whichever popover is open.
	useEffect(() => {
		if (!openSpawnId) return;
		function handleClick(e: MouseEvent) {
			if (
				spawnRowRef.current &&
				!spawnRowRef.current.contains(e.target as Node)
			) {
				setOpenSpawnId(null);
			}
		}
		document.addEventListener("pointerdown", handleClick);
		return () => document.removeEventListener("pointerdown", handleClick);
	}, [openSpawnId]);

	function handleSpawn(sidecarId: string) {
		onSpawn(sidecarId, command, args ? args.split(" ") : []);
		setOpenSpawnId(null);
	}

	function handleContextMenu(
		e: React.MouseEvent,
		sidecarId: string,
		pid: number,
	) {
		e.preventDefault();
		e.stopPropagation();
		onProcessContextMenu(sidecarId, pid, e.clientX, e.clientY);
	}

	return (
		<div className={s.dock} data-testid="dock">
			{/* App icons — one per process */}
			{processes.map((proc) => (
				<ViewTransition
					key={`${proc.sidecarId}:${proc.pid}`}
					enter="dock-icon-in"
					exit="dock-icon-out"
				>
					<Tooltip label={proc.title} side="top">
						<button
							type="button"
							className={s.iconButton}
							style={{ background: proc.color }}
							onClick={() => onFocusWindow(proc.sidecarId, proc.pid)}
							onContextMenu={(e) =>
								handleContextMenu(e, proc.sidecarId, proc.pid)
							}
							data-testid="process-icon"
							data-pid={proc.pid}
						>
							<span className={s.runningDot} />
							{proc.title.charAt(0).toUpperCase()}
						</button>
					</Tooltip>
				</ViewTransition>
			))}

			{/* Separator between apps and the per-sidecar add buttons */}
			{processes.length > 0 && sidecars.length > 0 && (
				<div className={s.separator} />
			)}

			{/* One + button per connected sidecar. */}
			<div ref={spawnRowRef} className={s.spawnRow}>
				{sidecars.map((sc) => {
					const isOpen = openSpawnId === sc.id;
					return (
						<div key={sc.id} className={s.spawnSlot}>
							<Tooltip label={sc.name} side="top">
								<button
									type="button"
									className={`${s.iconButton} ${s.addButton}`}
									onClick={() => {
										setOpenSpawnId(isOpen ? null : sc.id);
									}}
									data-testid="spawn-button"
									data-sidecar-id={sc.id}
								>
									<span className={s.statusDot}>
										<span
											className={`${s.statusDotInner} ${connected ? s.online : s.offline}`}
											data-testid="connection-status"
										/>
									</span>
									+
								</button>
							</Tooltip>

							{isOpen && (
								<div className={s.popoverStack}>
									{(() => {
										const sidecarWindows = windows.filter(
											(w) => w.sidecarId === sc.id,
										);
										if (sidecarWindows.length === 0) return null;
										return (
											<PolaroidStack>
												{sidecarWindows.map((w) => (
													<Polaroid
														key={w.windowId}
														src={thumbnails.get(w.windowId)}
														caption={w.title || "Untitled"}
														title={w.title || w.windowId}
														draggable
														onDragStart={(e) => {
															e.dataTransfer.setData(
																DOCK_WINDOW_DRAG_MIME,
																w.windowId,
															);
															e.dataTransfer.effectAllowed = "copy";
														}}
													/>
												))}
											</PolaroidStack>
										);
									})()}

									<div className={s.popover}>
										<div className={s.popoverLabel}>{sc.name}</div>
										<div className={s.popoverRow}>
											<input
												type="text"
												value={command}
												onChange={(e) => setCommand(e.target.value)}
												placeholder="command"
												className={s.popoverInput}
												onKeyDown={(e) => {
													if (e.key === "Enter") handleSpawn(sc.id);
												}}
												data-testid="spawn-command"
											/>
											<input
												type="text"
												value={args}
												onChange={(e) => setArgs(e.target.value)}
												placeholder="args"
												className={s.popoverInput}
												onKeyDown={(e) => {
													if (e.key === "Enter") handleSpawn(sc.id);
												}}
												data-testid="spawn-args"
											/>
											<button
												type="button"
												className={s.popoverButton}
												onClick={() => handleSpawn(sc.id)}
												data-testid="spawn-submit"
											>
												Spawn
											</button>
										</div>
									</div>
								</div>
							)}
						</div>
					);
				})}
			</div>
		</div>
	);
}
