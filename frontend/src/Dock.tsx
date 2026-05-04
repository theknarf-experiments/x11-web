import { useLiveQuery } from "@tanstack/react-db";
import { useEffect, useRef, useState, ViewTransition } from "react";
import { AppContextMenu, getAppContextMenuItems } from "./AppContextMenu";
import s from "./Dock.module.css";
import { sidecarsCollection, windowsCollection } from "./db";
import { Tooltip } from "./Tooltip";

export interface DockProcess {
	sidecarId: string;
	pid: number;
	title: string;
	color: string;
}

interface DockProps {
	connected: boolean;
	processes: DockProcess[];
	/// Per-window thumbnail object URLs (windowId → blob URL). The
	/// dock looks each window up here when rendering the picker so
	/// it can show a live preview alongside the spawn controls.
	thumbnails: Map<string, string>;
	/// Window IDs already attached to the active workspace's canvas.
	/// The picker hides these — once a polaroid is dragged out it
	/// turns into a live WindowFrame, no need to keep showing it as
	/// a thumbnail.
	attachedWindowIds: Set<string>;
	onSpawn: (sidecarId: string, command: string, args: string[]) => void;
	onClose: (sidecarId: string, pid: number) => void;
	onFocusWindow: (sidecarId: string, pid: number) => void;
}

export function Dock({
	connected,
	processes,
	thumbnails,
	attachedWindowIds,
	onSpawn,
	onClose,
	onFocusWindow,
}: DockProps) {
	const { data: sidecars = [] } = useLiveQuery((q) =>
		q.from({ s: sidecarsCollection }).select(({ s }) => s),
	);
	const { data: allWindows = [] } = useLiveQuery((q) =>
		q.from({ w: windowsCollection }).select(({ w }) => w),
	);
	const [openSpawnId, setOpenSpawnId] = useState<string | null>(null);
	const [contextMenu, setContextMenu] = useState<{
		sidecarId: string;
		pid: number;
		x: number;
		y: number;
	} | null>(null);
	const [command, setCommand] = useState("xeyes");
	const [args, setArgs] = useState("");
	const spawnRowRef = useRef<HTMLDivElement>(null);

	// Outside-click closes whichever popover is open. The context menu
	// has its own outside-click handler inside `AppContextMenu`.
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
						<Tooltip label={proc.title} side="top">
							<button
								type="button"
								className={s.iconButton}
								style={{ background: proc.color }}
								onClick={() => onFocusWindow(proc.sidecarId, proc.pid)}
								onContextMenu={(e) =>
									handleContextMenu(e, proc.sidecarId, proc.pid)
								}
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
											setContextMenu(null);
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
											const sidecarWindows = allWindows.filter(
												(w) =>
													w.sidecarId === sc.id &&
													!attachedWindowIds.has(w.windowId),
											);
											if (sidecarWindows.length === 0) return null;
											return (
												<div className={s.thumbnailGrid}>
													{sidecarWindows.map((w) => {
														const url = thumbnails.get(w.windowId);
														return (
															<div
																key={w.windowId}
																className={s.thumbnailCell}
																title={w.title || w.windowId}
																draggable={true}
																onDragStart={(e) => {
																	e.dataTransfer.setData(
																		"application/x-x11web-window-id",
																		w.windowId,
																	);
																	e.dataTransfer.effectAllowed = "copy";
																}}
															>
																{url ? (
																	<img
																		src={url}
																		alt={w.title || ""}
																		className={s.thumbnailImg}
																		draggable={false}
																	/>
																) : (
																	<div className={s.thumbnailPlaceholder} />
																)}
																<div className={s.thumbnailTitle}>
																	{w.title || "Untitled"}
																</div>
															</div>
														);
													})}
												</div>
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
											/>
											<button
												type="button"
												className={s.popoverButton}
												onClick={() => handleSpawn(sc.id)}
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
