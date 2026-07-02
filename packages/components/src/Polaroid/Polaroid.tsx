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
		<div
			className={s.polaroid}
			data-testid="polaroid"
			title={title ?? caption}
			draggable={draggable}
			onDragStart={onDragStart}
			onClick={onClick}
		>
			{src ? (
				<img src={src} alt={caption} className={s.image} draggable={false} />
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
