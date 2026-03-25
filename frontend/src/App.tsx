import { useCallback, useEffect, useRef, useState } from "react";
import s from "./App.module.css";
import { Button } from "./components/Button";
import type { DisplayUpdate, InputEvent } from "./types";
import { useBackendSocket } from "./useBackendSocket";
import { X11Canvas } from "./X11Canvas";

let requestCounter = 0;
function nextRequestId() {
	return `req-${++requestCounter}-${Date.now()}`;
}

function App() {
	const { connected, sidecars, processes, send, onDisplayUpdate } =
		useBackendSocket();
	const [command, setCommand] = useState("xeyes");
	const [args, setArgs] = useState("");
	const [viewingSidecar, setViewingSidecar] = useState<string | null>(null);
	const [killingPids, setKillingPids] = useState<Set<number>>(new Set());

	// Display update queue — shared between WebSocket callback and canvas rAF loop
	const displayQueueRef = useRef<DisplayUpdate[]>([]);
	const viewingSidecarRef = useRef(viewingSidecar);
	viewingSidecarRef.current = viewingSidecar;

	// Register display callback: push updates for the viewed sidecar into the queue
	useEffect(() => {
		onDisplayUpdate((sidecarId, update) => {
			if (sidecarId === viewingSidecarRef.current) {
				displayQueueRef.current.push(update);
			}
		});
		return () => onDisplayUpdate(null);
	}, [onDisplayUpdate]);

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
			if (viewingSidecar) {
				send({
					type: "InputEvent",
					sidecar_id: viewingSidecar,
					window_id: 0,
					event,
				});
			}
		},
		[viewingSidecar, send],
	);

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

				{viewingSidecar && (
					<section className={s.displaySection} data-testid="display-section">
						<h2>
							Display —{" "}
							{sidecars.find((sc) => sc.id === viewingSidecar)?.name ??
								viewingSidecar.slice(0, 8)}
						</h2>
						<X11Canvas
							queueRef={displayQueueRef}
							width={1024}
							height={768}
							onInput={handleInput}
						/>
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
