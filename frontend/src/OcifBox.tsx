import { useCallback } from "react";
import s from "./OcifBox.module.css";
import type { OcifNode } from "./workspaceSync";

interface OcifBoxProps {
	id: string;
	node: OcifNode;
	selected: boolean;
	/** Pointer-down on the box body. App.tsx uses this to drive
	 *  click-to-select and drag-to-move. */
	onPointerDown: (id: string, e: React.PointerEvent) => void;
}

const DEFAULT_FILL = "transparent";
const DEFAULT_STROKE = "#ffffff";
const DEFAULT_STROKE_WIDTH = 2;
const DEFAULT_RADIUS = 6;

/** One `@ocif/rect` node rendered as an absolute-positioned div in
 *  canvas space. Uses `border` + `box-sizing: border-box` so the
 *  rendered outer width / height match the doc-stored OCIF size
 *  exactly (the border draws *inside* the box, not outside). The
 *  border-radius is a render-time treatment — not part of the
 *  `@ocif/rect` spec — so it stays in CSS rather than the doc. */
export function OcifBox({ id, node, selected, onPointerDown }: OcifBoxProps) {
	const handlePointerDown = useCallback(
		(e: React.PointerEvent) => {
			e.stopPropagation();
			onPointerDown(id, e);
		},
		[id, onPointerDown],
	);
	const fill = node.rect?.fill_color ?? DEFAULT_FILL;
	const stroke = node.rect?.stroke_color ?? DEFAULT_STROKE;
	const strokeWidth = node.rect?.stroke_width ?? DEFAULT_STROKE_WIDTH;
	return (
		<div
			data-testid="ocif-box"
			data-node-id={id}
			className={selected ? s.selected : s.box}
			style={{
				position: "absolute",
				left: node.x,
				top: node.y,
				width: node.width,
				height: node.height,
				zIndex: Math.round(node.z),
				background: fill,
				border: `${strokeWidth}px solid ${stroke}`,
				borderRadius: DEFAULT_RADIUS,
			}}
			onPointerDown={handlePointerDown}
		/>
	);
}
