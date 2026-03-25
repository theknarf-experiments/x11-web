import { useEffect, useRef, useState } from "react";
import s from "./Dock.module.css";
import type { SidecarInfo } from "./types";

interface DockWindow {
	clientId: string;
	sidecarId: string;
	title: string;
	color: string;
}

interface DockProps {
	connected: boolean;
	sidecars: SidecarInfo[];
	windows: DockWindow[];
	onSpawn: (sidecarId: string, command: string, args: string[]) => void;
	onClose: (clientId: string) => void;
	onFocusWindow: (clientId: string) => void;
}

export function Dock({
	connected,
	sidecars,
	windows,
	onSpawn,
	onClose,
	onFocusWindow,
}: DockProps) {
	const [showSpawn, setShowSpawn] = useState(false);
	const [contextMenu, setContextMenu] = useState<{
		clientId: string;
		x: number;
		y: number;
	} | null>(null);
	const [command, setCommand] = useState("xeyes");
	const [args, setArgs] = useState("");
	const [selectedSidecar, setSelectedSidecar] = useState<string>("");
	const contextRef = useRef<HTMLDivElement>(null);
	const spawnRef = useRef<HTMLDivElement>(null);

	// Auto-select first sidecar
	useEffect(() => {
		if (!selectedSidecar && sidecars.length > 0) {
			setSelectedSidecar(sidecars[0].id);
		}
	}, [sidecars, selectedSidecar]);

	// Close menus on outside click
	useEffect(() => {
		function handleClick(e: MouseEvent) {
			if (
				contextMenu &&
				contextRef.current &&
				!contextRef.current.contains(e.target as Node)
			) {
				setContextMenu(null);
			}
			if (
				showSpawn &&
				spawnRef.current &&
				!spawnRef.current.contains(e.target as Node)
			) {
				setShowSpawn(false);
			}
		}
		document.addEventListener("pointerdown", handleClick);
		return () => document.removeEventListener("pointerdown", handleClick);
	}, [contextMenu, showSpawn]);

	function handleSpawn() {
		if (!selectedSidecar) return;
		onSpawn(selectedSidecar, command, args ? args.split(" ") : []);
		setShowSpawn(false);
	}

	function handleContextMenu(e: React.MouseEvent, clientId: string) {
		e.preventDefault();
		e.stopPropagation();
		setContextMenu({ clientId, x: e.clientX, y: e.clientY });
	}

	return (
		<div className={s.dock} data-testid="dock">
			{/* App icons */}
			{windows.map((win) => (
				<button
					key={win.clientId}
					type="button"
					className={s.iconButton}
					style={{ background: win.color }}
					onClick={() => onFocusWindow(win.clientId)}
					onContextMenu={(e) => handleContextMenu(e, win.clientId)}
				>
					<span className={s.tooltip}>{win.title}</span>
					<span className={s.runningDot} />
					{win.title.charAt(0).toUpperCase()}
				</button>
			))}

			{/* Separator between apps and add button */}
			{windows.length > 0 && <div className={s.separator} />}

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

			{/* Right-click context menu */}
			{contextMenu && (
				<div
					ref={contextRef}
					className={s.contextMenu}
					style={{
						left: contextMenu.x,
						bottom: `calc(100vh - ${contextMenu.y}px)`,
					}}
				>
					<button
						type="button"
						className={s.contextMenuItem}
						onClick={() => {
							onClose(contextMenu.clientId);
							setContextMenu(null);
						}}
					>
						Close
					</button>
				</div>
			)}
		</div>
	);
}
