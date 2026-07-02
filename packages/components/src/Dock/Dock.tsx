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

/** Superellipse |x/r|^n + |y/r|^n = 1 sampled into a closed SVG
 *  path. n=5 is the Apple "squircle" exponent; rendered as an inline
 *  SVG (rather than CSS `corner-shape`) so every browser gets the
 *  continuous-corner shape. */
function superellipsePath(size: number, n = 5, steps = 128): string {
	const r = size / 2;
	const pts: string[] = [];
	for (let i = 0; i < steps; i++) {
		const t = (i / steps) * 2 * Math.PI;
		const c = Math.cos(t);
		const s = Math.sin(t);
		const x = r + r * Math.sign(c) * Math.abs(c) ** (2 / n);
		const y = r + r * Math.sign(s) * Math.abs(s) ** (2 / n);
		pts.push(`${x.toFixed(2)} ${y.toFixed(2)}`);
	}
	return `M${pts.join("L")}Z`;
}

const ICON_SIZE = 48;
const SQUIRCLE_PATH = superellipsePath(ICON_SIZE);

/** Peak scale of the icon directly under the cursor. */
const MAGNIFY_MAX = 1.5;
/** Horizontal falloff distance (px) on either side of the cursor. */
const MAGNIFY_RANGE = 96;

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
	const dockRef = useRef<HTMLDivElement>(null);

	/** macOS-style magnification: each icon scales by a cos² falloff
	 *  of its horizontal distance to the cursor. Applied imperatively
	 *  via the `--magnify` custom property so pointermove doesn't
	 *  re-render React; CSS transitions smooth the motion. */
	function applyMagnify(mouseX: number | null) {
		const dock = dockRef.current;
		if (!dock) return;
		for (const el of dock.querySelectorAll<HTMLElement>("[data-magnify]")) {
			if (mouseX === null) {
				el.style.removeProperty("--magnify");
				continue;
			}
			const rect = el.getBoundingClientRect();
			const dist = Math.abs(mouseX - (rect.left + rect.width / 2));
			if (dist > MAGNIFY_RANGE) {
				el.style.removeProperty("--magnify");
				continue;
			}
			const scale =
				1 +
				(MAGNIFY_MAX - 1) *
					Math.cos((dist / MAGNIFY_RANGE) * (Math.PI / 2)) ** 2;
			el.style.setProperty("--magnify", scale.toFixed(3));
		}
	}

	function handleDockPointerMove(e: React.PointerEvent) {
		if (e.pointerType === "touch") return;
		// The spawn popover is a child of the dock but floats above the
		// bar — don't magnify while the cursor is up there.
		const rect = dockRef.current?.getBoundingClientRect();
		if (rect && e.clientY < rect.top) {
			applyMagnify(null);
			return;
		}
		applyMagnify(e.clientX);
	}

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
		<div
			ref={dockRef}
			className={s.dock}
			data-testid="dock"
			onPointerMove={handleDockPointerMove}
			onPointerLeave={() => applyMagnify(null)}
		>
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
							onClick={() => onFocusWindow(proc.sidecarId, proc.pid)}
							onContextMenu={(e) =>
								handleContextMenu(e, proc.sidecarId, proc.pid)
							}
							data-testid="process-icon"
							data-pid={proc.pid}
							data-magnify
						>
							<svg
								className={s.iconShape}
								viewBox={`0 0 ${ICON_SIZE} ${ICON_SIZE}`}
								aria-hidden="true"
							>
								<path d={SQUIRCLE_PATH} fill={proc.color} />
							</svg>
							<span className={s.runningDot} />
							<span className={s.glyph}>
								{proc.title.charAt(0).toUpperCase()}
							</span>
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
									data-magnify
								>
									<svg
										className={s.iconShape}
										viewBox={`0 0 ${ICON_SIZE} ${ICON_SIZE}`}
										aria-hidden="true"
									>
										<path d={SQUIRCLE_PATH} />
									</svg>
									<span className={s.statusDot}>
										<span
											className={`${s.statusDotInner} ${connected ? s.online : s.offline}`}
											data-testid="connection-status"
										/>
									</span>
									<span className={s.glyph}>+</span>
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
