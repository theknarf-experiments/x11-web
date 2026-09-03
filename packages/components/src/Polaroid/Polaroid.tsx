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
	onDragStart?: (e: React.DragEvent<HTMLButtonElement>) => void;
	onClick?: (e: React.MouseEvent<HTMLButtonElement>) => void;
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
		// A real <button>, not a div with an onClick: the card is a
		// picker item, so it has to be reachable by Tab and activatable
		// by Enter/Space. Doing that by hand (role + tabIndex + a key
		// handler) reimplements what the element already does, and gets
		// the Space-scrolls-the-page case wrong. `.polaroid` resets the
		// UA button chrome.
		<button
			type="button"
			className={s.polaroid}
			data-testid="polaroid"
			title={title ?? caption}
			draggable={draggable}
			onDragStart={onDragStart}
			onClick={onClick}
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
		</button>
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
