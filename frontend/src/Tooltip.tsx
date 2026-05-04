import s from "./Tooltip.module.css";

export type TooltipSide = "top" | "right" | "bottom" | "left";

interface TooltipProps {
	/** Plain-text label rendered in the bubble. */
	label: string;
	/** Optional keyboard shortcut shown as a small chip on the right
	 *  of the label. Use single keys ("V") or modifier strings ("⌘S"). */
	hotkey?: string;
	/** Which side of the trigger the bubble sits on. Defaults to top. */
	side?: TooltipSide;
	/** The trigger — typically a button or icon. */
	children: React.ReactNode;
}

/** Wraps a trigger element with a hover/focus tooltip. The wrapper
 *  becomes the positioning context (`position: relative`); the bubble
 *  is an absolutely-positioned sibling of the trigger that fades in
 *  on `:hover` or `:focus-within` after a short delay.
 *
 *  Accessibility: pass `aria-label` (and `aria-keyshortcuts` if a
 *  hotkey is set) on the trigger child — the bubble is purely
 *  visual. The component doesn't enforce that, since some triggers
 *  (e.g., labelled buttons) read fine on their own. */
export function Tooltip({
	label,
	hotkey,
	side = "top",
	children,
}: TooltipProps) {
	const sideClass =
		side === "right"
			? s.right
			: side === "left"
				? s.left
				: side === "bottom"
					? s.bottom
					: s.top;
	return (
		<span className={s.wrapper}>
			{children}
			<span className={`${s.bubble} ${sideClass}`} role="presentation">
				<span className={s.label}>{label}</span>
				{hotkey && <span className={s.key}>{hotkey}</span>}
			</span>
		</span>
	);
}
