import { type ReactNode, useEffect, useRef, useState } from "react";
import s from "./Popover.module.css";

export type PopoverSide = "top" | "right" | "bottom" | "left";
/** Cross-axis edge of the panel that lines up with the trigger.
 *  For `top` / `bottom` sides: `start` = left, `end` = right.
 *  For `left` / `right` sides:  `start` = top,  `end` = bottom. */
export type PopoverAlign = "start" | "end";

interface PopoverProps {
	/** Whatever element the user clicks to open the popover —
	 *  typically a styled `<button>`. The popover handles the
	 *  click-to-toggle itself; the trigger's own `onClick` (if any)
	 *  still fires alongside. */
	trigger: ReactNode;
	/** Side of the trigger the panel anchors to. Default `top`. */
	side?: PopoverSide;
	/** Cross-axis edge alignment. Default `start`. Use `end` when
	 *  the trigger sits near a viewport edge so the panel grows
	 *  back into the viewport instead of overflowing. */
	align?: PopoverAlign;
	/** Class merged onto the popover panel — for sizing / chrome
	 *  beyond the default (no background, no padding). */
	className?: string;
	/** Panel content. Pass a function to receive a `close` callback —
	 *  useful for "click-an-item-to-close" menus. */
	children: ReactNode | ((p: { close: () => void }) => ReactNode);
}

/** Click-to-toggle popover: renders the trigger inline, opens an
 *  absolutely-positioned panel anchored to one of its sides on
 *  click, dismisses on outside-click. The popover ships no panel
 *  chrome of its own — pass a `className` (CSS module or plain) if
 *  you want background / padding / border. */
export function Popover({
	trigger,
	side = "top",
	align = "start",
	className,
	children,
}: PopoverProps) {
	const [open, setOpen] = useState(false);
	const wrapRef = useRef<HTMLDivElement>(null);

	// Outside-click dismisses. Listening on `pointerdown` (rather
	// than `click`) means the popover closes the moment the user
	// presses outside, even before they release.
	useEffect(() => {
		if (!open) return;
		function onPointerDown(e: PointerEvent) {
			if (!wrapRef.current?.contains(e.target as Node)) {
				setOpen(false);
			}
		}
		document.addEventListener("pointerdown", onPointerDown);
		return () => document.removeEventListener("pointerdown", onPointerDown);
	}, [open]);

	const sideClass =
		side === "right"
			? s.right
			: side === "left"
				? s.left
				: side === "bottom"
					? s.bottom
					: s.top;
	const alignClass = align === "end" ? s.end : s.start;
	const close = () => setOpen(false);

	return (
		<div ref={wrapRef} className={s.wrap} data-popover-open={open || undefined}>
			<span className={s.trigger} onClick={() => setOpen((o) => !o)}>
				{trigger}
			</span>
			{open && (
				<div
					className={`${s.panel} ${sideClass} ${alignClass} ${className ?? ""}`}
					role="dialog"
					data-testid="popover-panel"
				>
					{typeof children === "function" ? children({ close }) : children}
				</div>
			)}
		</div>
	);
}
