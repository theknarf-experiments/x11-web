import { useState } from "react";
import s from "./DiagnosticsPanel.module.css";
import type { Diagnostic } from "./useBackendSocket";

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
 * the backend / sidecar / WebSocket layer (input drops, command results,
 * connection errors). Collapsed by default; the badge counts unseen
 * entries since the last open.
 */
export function DiagnosticsPanel({
	diagnostics,
	onDismiss,
	onClear,
}: DiagnosticsPanelProps) {
	const [open, setOpen] = useState(false);

	if (diagnostics.length === 0 && !open) return null;

	const errorCount = diagnostics.filter((d) => d.level === "error").length;
	const warnCount = diagnostics.filter((d) => d.level === "warn").length;
	const badgeLevel: Diagnostic["level"] =
		errorCount > 0 ? "error" : warnCount > 0 ? "warn" : "info";

	return (
		<div className={s.root} data-testid="diagnostics-panel">
			{open && (
				<div className={s.panel}>
					<div className={s.header}>
						<span className={s.title}>Diagnostics</span>
						<div className={s.headerActions}>
							{diagnostics.length > 0 && (
								<button
									type="button"
									className={s.headerButton}
									onClick={onClear}
								>
									Clear
								</button>
							)}
							<button
								type="button"
								className={s.headerButton}
								onClick={() => setOpen(false)}
							>
								×
							</button>
						</div>
					</div>
					<div className={s.list}>
						{diagnostics.length === 0 && (
							<div className={s.empty}>No diagnostics</div>
						)}
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
									<span className={s.entryTime}>{formatTime(d.timestamp)}</span>
									<span className={s.entrySource}>{d.source}</span>
									<span className={s.entryMessage}>{d.message}</span>
									{d.windowId && (
										<span className={s.entryContext}>
											win {d.windowId.slice(0, 8)}
										</span>
									)}
								</button>
							))}
					</div>
				</div>
			)}
			<button
				type="button"
				className={`${s.toggle} ${s[badgeLevel]}`}
				onClick={() => setOpen((v) => !v)}
				data-testid="diagnostics-toggle"
			>
				<span className={s.toggleDot} />
				{diagnostics.length}
			</button>
		</div>
	);
}
