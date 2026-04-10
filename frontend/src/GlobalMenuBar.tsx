import s from "./GlobalMenuBar.module.css";

interface GlobalMenuBarProps {
	/** Title of the currently focused window, or null when nothing is focused. */
	focusedTitle: string | null;
}

/**
 * macOS-style global menu bar fixed to the top of the page.
 *
 * Phase 0 scaffolding: shows the focused window's title only. Real
 * menu rendering will land in PR 2 once GTK menu mirroring is wired
 * through the sidecar's DBus client.
 */
export function GlobalMenuBar({ focusedTitle }: GlobalMenuBarProps) {
	return (
		<div className={s.menuBar} data-testid="global-menu-bar">
			<span className={s.appTitle} data-testid="global-menu-bar-title">
				{focusedTitle ?? "x11-web"}
			</span>
		</div>
	);
}
