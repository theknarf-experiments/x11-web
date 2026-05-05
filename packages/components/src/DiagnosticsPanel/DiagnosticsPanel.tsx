import { Popover } from "../Popover/Popover.tsx";
import s from "./DiagnosticsPanel.module.css";

/** A single diagnostic entry — the shape consumed by the panel.
 *  Lives here (not next to the producer) so the producer code in
 *  the host app can import the type from the same place that
 *  renders it. */
export interface Diagnostic {
	id: string;
	level: "info" | "warn" | "error";
	source: "ws" | "command" | "sidecar";
	message: string;
	timestamp: number;
}

interface DiagnosticsPanelProps {
	diagnostics: Diagnostic[];
	onDismiss: (id: string) => void;
	onClear: () => void;
}

function formatTime(ts: number): string {
	const d = new Date(ts);
	const hh = String(d.getHours()).padStart(2, "0");
	const mm = String(d.getMinutes()).padStart(2, "0");
	const ss = String(d.getSeconds()).padStart(2, "0");
	return `${hh}:${mm}:${ss}`;
}

/**
 * Bottom-right collapsible log strip showing diagnostics surfaced from
 * the backend / sidecar / WebSocket layer (input drops, command
 * results, connection errors). The toggle's badge dot reflects the
 * highest-severity entry currently in the list.
 */
export function DiagnosticsPanel({
	diagnostics,
	onDismiss,
	onClear,
}: DiagnosticsPanelProps) {
	if (diagnostics.length === 0) return null;

	const errorCount = diagnostics.filter((d) => d.level === "error").length;
	const warnCount = diagnostics.filter((d) => d.level === "warn").length;
	const badgeLevel: Diagnostic["level"] =
		errorCount > 0 ? "error" : warnCount > 0 ? "warn" : "info";

	return (
		<div className={s.root} data-testid="diagnostics-panel">
			<Popover
				side="top"
				align="end"
				className={s.panel}
				trigger={
					<button
						type="button"
						className={`${s.toggle} ${s[badgeLevel]}`}
						data-testid="diagnostics-toggle"
					>
						<span className={s.toggleDot} />
						{diagnostics.length}
					</button>
				}
			>
				{({ close }) => (
					<>
						<div className={s.header}>
							<span className={s.title}>Diagnostics</span>
							<div className={s.headerActions}>
								<button
									type="button"
									className={s.headerButton}
									onClick={onClear}
								>
									Clear
								</button>
								<button
									type="button"
									className={s.headerButton}
									onClick={close}
								>
									×
								</button>
							</div>
						</div>
						<div className={s.list}>
							{diagnostics
								.slice()
								.reverse()
								.map((d) => (
									<button
										type="button"
										key={d.id}
										className={`${s.entry} ${s[d.level]}`}
										onClick={() => onDismiss(d.id)}
										title="Click to dismiss"
									>
										<span className={s.entryTime}>
											{formatTime(d.timestamp)}
										</span>
										<span className={s.entrySource}>{d.source}</span>
										<span className={s.entryMessage}>{d.message}</span>
									</button>
								))}
						</div>
					</>
				)}
			</Popover>
		</div>
	);
}
