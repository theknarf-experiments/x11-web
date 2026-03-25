import { useCallback, useEffect, useRef, useState } from "react";
import s from "./App.module.css";
import { ClientRenderer } from "./ClientRenderer";
import { Button } from "./components/Button";
import type { InputEvent } from "./types";
import { useBackendSocket } from "./useBackendSocket";
import { X11Canvas } from "./X11Canvas";

let requestCounter = 0;
function nextRequestId() {
	return `req-${++requestCounter}-${Date.now()}`;
}

function App() {
	const {
		connected,
		sidecars,
		processes,
		connectedProcesses,
		send,
		onDisplayUpdate,
	} = useBackendSocket();
	const [command, setCommand] = useState("xeyes");
	const [args, setArgs] = useState("");
	const [viewingSidecar, setViewingSidecar] = useState<string | null>(null);
	const [activeClientId, setActiveClientId] = useState<string | null>(null);
	const [killingPids, setKillingPids] = useState<Set<number>>(new Set());

	// Per-client_id renderers (persistent back buffers, survive tab switches)
	const renderersRef = useRef<Map<string, ClientRenderer>>(new Map());
	const viewingSidecarRef = useRef(viewingSidecar);
	viewingSidecarRef.current = viewingSidecar;

	function getRenderer(clientId: string): ClientRenderer {
		const renderers = renderersRef.current;
		let r = renderers.get(clientId);
		if (!r) {
			r = new ClientRenderer(1024, 768);
			renderers.set(clientId, r);
		}
		return r;
	}

	// Register display callback: render updates directly to per-client back buffers
	useEffect(() => {
		onDisplayUpdate((sidecarId, clientId, update) => {
			if (sidecarId === viewingSidecarRef.current) {
				const renderers = renderersRef.current;
				let r = renderers.get(clientId);
				if (!r) {
					r = new ClientRenderer(1024, 768);
					renderers.set(clientId, r);
				}
				r.pushUpdate(update);
			}
		});
		return () => onDisplayUpdate(null);
	}, [onDisplayUpdate]);

	// Auto-select first connected process as active tab
	useEffect(() => {
		if (!activeClientId && connectedProcesses.length > 0) {
			setActiveClientId(connectedProcesses[0].clientId);
		}
	}, [activeClientId, connectedProcesses]);

	function handleSpawn(sidecarId: string) {
		send({ type: "SubscribeDisplay", sidecar_id: sidecarId });
		setViewingSidecar(sidecarId);

		send({
			type: "SpawnProcess",
			request_id: nextRequestId(),
			sidecar_id: sidecarId,
			command,
			args: args ? args.split(" ") : [],
		});

		setTimeout(() => {
			send({
				type: "ListProcesses",
				request_id: nextRequestId(),
				sidecar_id: sidecarId,
			});
		}, 500);
	}

	function handleKill(sidecarId: string, pid: number) {
		setKillingPids((prev) => new Set(prev).add(pid));
		send({
			type: "KillProcess",
			request_id: nextRequestId(),
			sidecar_id: sidecarId,
			pid,
		});
	}

	function handleListProcesses(sidecarId: string) {
		send({
			type: "ListProcesses",
			request_id: nextRequestId(),
			sidecar_id: sidecarId,
		});
	}

	function handleViewDisplay(sidecarId: string) {
		send({ type: "SubscribeDisplay", sidecar_id: sidecarId });
		setViewingSidecar(sidecarId);
	}

	const handleInput = useCallback(
		(event: InputEvent) => {
			if (viewingSidecar && activeClientId) {
				send({
					type: "InputEvent",
					sidecar_id: viewingSidecar,
					client_id: activeClientId,
					event,
				});
			}
		},
		[viewingSidecar, activeClientId, send],
	);

	// Get processes for the viewed sidecar that have connected X11 clients
	const sidecarProcesses = connectedProcesses.filter(
		(p) => p.sidecarId === viewingSidecar,
	);

	// Find the command name for a connected process
	function processLabel(cp: { pid: number; sidecarId: string }) {
		const procList = processes[cp.sidecarId] || [];
		const proc = procList.find((p) => p.pid === cp.pid);
		return proc ? `${proc.command} (${cp.pid})` : `PID ${cp.pid}`;
	}

	// Active renderer for the canvas
	const activeRenderer = activeClientId ? getRenderer(activeClientId) : null;

	return (
		<div className={s.app}>
			<header className={s.header}>
				<h1>x11-web</h1>
				<span
					className={`${s.status} ${connected ? s.online : s.offline}`}
					data-testid="connection-status"
				>
					{connected ? "Connected" : "Disconnected"}
				</span>
			</header>

			<main>
				<section className={s.launchControls}>
					<h2>Launch Application</h2>
					<div className={s.formRow}>
						<label>
							Command:
							<input
								type="text"
								value={command}
								onChange={(e) => setCommand(e.target.value)}
								placeholder="e.g. xeyes"
							/>
						</label>
						<label>
							Args:
							<input
								type="text"
								value={args}
								onChange={(e) => setArgs(e.target.value)}
								placeholder="e.g. -geometry 200x200"
							/>
						</label>
					</div>
				</section>

				{viewingSidecar && sidecarProcesses.length > 0 && (
					<section className={s.displaySection} data-testid="display-section">
						<div className={s.tabs} data-testid="process-tabs">
							{sidecarProcesses.map((cp) => (
								<button
									key={cp.clientId}
									type="button"
									className={`${s.tab} ${cp.clientId === activeClientId ? s.tabActive : ""}`}
									onClick={() => setActiveClientId(cp.clientId)}
									data-testid="process-tab"
								>
									{processLabel(cp)}
								</button>
							))}
						</div>
						{activeRenderer && (
							<X11Canvas
								key={activeClientId}
								renderer={activeRenderer}
								width={1024}
								height={768}
								onInput={handleInput}
							/>
						)}
					</section>
				)}

				<section className={s.sidecars}>
					<h2>Sidecars ({sidecars.length})</h2>
					{sidecars.length === 0 ? (
						<p className={s.empty}>
							No sidecars connected. Start a sidecar to begin.
						</p>
					) : (
						sidecars.map((sidecar) => (
							<div
								key={sidecar.id}
								className={s.sidecarCard}
								data-testid="sidecar-card"
							>
								<div className={s.sidecarHeader}>
									<h3>{sidecar.name}</h3>
									<code>{sidecar.id.slice(0, 8)}</code>
								</div>
								<div className={s.sidecarActions}>
									<Button onClick={() => handleSpawn(sidecar.id)}>
										Spawn {command}
									</Button>
									<Button onClick={() => handleViewDisplay(sidecar.id)}>
										View Display
									</Button>
									<Button onClick={() => handleListProcesses(sidecar.id)}>
										Refresh
									</Button>
								</div>
								<div className={s.processList}>
									<h4>Processes</h4>
									{(processes[sidecar.id] || []).length === 0 ? (
										<p className={s.empty}>No processes running</p>
									) : (
										<ul>
											{(processes[sidecar.id] || []).map((proc) => {
												const isKilling = killingPids.has(proc.pid);
												return (
													<li
														key={proc.pid}
														className={isKilling ? s.killing : ""}
													>
														<span>
															PID {proc.pid} — {proc.command}
															{isKilling && (
																<span className={s.killingLabel}>
																	{" "}
																	(stopping...)
																</span>
															)}
														</span>
														<Button
															variant="danger"
															onClick={() => handleKill(sidecar.id, proc.pid)}
															disabled={isKilling}
														>
															{isKilling ? "Stopping" : "Kill"}
														</Button>
													</li>
												);
											})}
										</ul>
									)}
								</div>
							</div>
						))
					)}
				</section>
			</main>
		</div>
	);
}

export default App;
