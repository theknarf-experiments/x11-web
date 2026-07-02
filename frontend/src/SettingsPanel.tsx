import { useState } from "react";
import type { FocusPolicy } from "./types";
import s from "./WindowFrame.module.css";

interface SettingsPanelProps {
	focusPolicy: FocusPolicy;
	onFocusPolicyChange: (policy: FocusPolicy) => void;
}

export function SettingsPanel({
	focusPolicy,
	onFocusPolicyChange,
}: SettingsPanelProps) {
	const [open, setOpen] = useState(false);

	if (!open) {
		return (
			<button
				type="button"
				onClick={() => setOpen(true)}
				style={{
					position: "fixed",
					bottom: 80,
					right: 16,
					background: "rgba(30,30,30,0.8)",
					border: "1px solid rgba(255,255,255,0.15)",
					borderRadius: 6,
					color: "#ccc",
					padding: "4px 10px",
					fontSize: 12,
					cursor: "pointer",
					zIndex: 20000,
				}}
				data-testid="settings-toggle"
			>
				Settings
			</button>
		);
	}

	return (
		<div className={s.settingsPanel} data-testid="settings-panel">
			<h4>
				Settings{" "}
				<button
					type="button"
					onClick={() => setOpen(false)}
					style={{
						float: "right",
						background: "none",
						border: "none",
						color: "#999",
						cursor: "pointer",
						fontSize: 14,
					}}
				>
					x
				</button>
			</h4>
			<div className={s.settingsRow}>
				<label htmlFor="focus-policy">Focus policy</label>
				<select
					id="focus-policy"
					value={focusPolicy}
					onChange={(e) => onFocusPolicyChange(e.target.value as FocusPolicy)}
				>
					<option value="click-to-focus">Click to focus</option>
					<option value="focus-follows-mouse">Focus follows mouse</option>
				</select>
			</div>
		</div>
	);
}
