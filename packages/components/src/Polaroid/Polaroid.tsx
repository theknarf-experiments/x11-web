import type { ReactNode } from "react";
import s from "./Polaroid.module.css";

interface PolaroidProps {
	/** Image URL. When omitted, a hatched placeholder renders in
	 *  the photo slot — useful while the source is loading or
	 *  unavailable. */
	src?: string;
	/** Handwritten-style caption shown below the photo. */
	caption: string;
	/** Native `title` tooltip on the card. */
	title?: string;
	/** Pass `true` to make the card a HTML5 drag source. The
	 *  caller wires `onDragStart` to attach payload + effect. */
	draggable?: boolean;
	onDragStart?: (e: React.DragEvent<HTMLDivElement>) => void;
	onClick?: (e: React.MouseEvent<HTMLDivElement>) => void;
}

/** A paper-textured "polaroid" card — image + handwritten caption
 *  on a slightly-yellowed background with a soft drop shadow.
 *  Standalone polaroids render flat; nest several inside
 *  `<PolaroidStack>` to get the fanned-tilt picker UX. */
export function Polaroid({
	src,
	caption,
	title,
	draggable,
	onDragStart,
	onClick,
}: PolaroidProps) {
	return (
		// A <div role="button">, NOT a real <button>, and that is
		// load-bearing rather than lazy: this card is the drag source for
		// the only way to attach a sidecar window to the canvas (see
		// Dock.tsx's onDragStart), and Gecko does not implement native
		// drag-and-drop on form controls — `dragstart` never fires on a
		// `<button draggable="true">` in Firefox. Making it a button
		// silently removes the drag path there, and nothing catches it:
		// the story dispatches a synthetic DragEvent (which fires on any
		// element) and the e2e suite is Chromium-only.
		//
		// The keyboard affordance a real button would have given us is
		// reimplemented below — tabIndex plus an Enter/Space handler —
		// which is exactly what biome's useKeyWithClickEvents asks for.
		// `preventDefault` on Space is what stops the page scrolling.
		//
		// biome-ignore lint/a11y/useSemanticElements: a real <button> is what the rule asks for and is exactly what Gecko refuses to start a drag from
		<div
			role="button"
			tabIndex={0}
			className={s.polaroid}
			data-testid="polaroid"
			title={title ?? caption}
			draggable={draggable}
			onDragStart={onDragStart}
			onClick={onClick}
			onKeyDown={(e) => {
				if (e.key !== "Enter" && e.key !== " ") return;
				e.preventDefault();
				onClick?.(e as unknown as React.MouseEvent<HTMLDivElement>);
			}}
		>
			{src ? (
				// `alt=""`: the card's own accessible name is the caption (via
				// the caption text and the `title`), so repeating it on the
				// photo makes a screen reader announce it twice — axe's
				// image-redundant-alt. The photo carries no information the
				// caption does not.
				<img src={src} alt="" className={s.image} draggable={false} />
			) : (
				<div className={s.placeholder} data-testid="polaroid-placeholder" />
			)}
			<div className={s.caption}>{caption}</div>
		</div>
	);
}

interface PolaroidStackProps {
	children: ReactNode;
}

/** Horizontal fan of `Polaroid` cards. Children must be direct
 *  `Polaroid` siblings — the tilt / offset cycles are driven by
 *  `:nth-child` on the stack's immediate descendants, so wrapping
 *  individual cards in extra elements would defeat them. */
export function PolaroidStack({ children }: PolaroidStackProps) {
	return (
		<div className={s.stack} data-testid="polaroid-stack">
			{children}
		</div>
	);
}
