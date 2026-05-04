import { useCallback } from "react";
import s from "./OcifBox.module.css";
import { OcifTextLayer } from "./OcifTextLayer";
import type { OcifNode } from "./workspaceSync";

export type ResizeHandle =
	| "n"
	| "s"
	| "e"
	| "w"
	| "ne"
	| "nw"
	| "se"
	| "sw";

interface OcifBoxProps {
	id: string;
	node: OcifNode;
	selected: boolean;
	editing: boolean;
	/** When true (pointer mode) the box intercepts pointerdown to
	 *  drive select / drag-to-move. When false (box / arrow draw
	 *  mode) the event bubbles up to the canvas-level handler so a
	 *  drag gesture can start ON TOP OF the box (e.g., drawing an
	 *  arrow from one box to another). */
	interactive: boolean;
	/** True when an arrow endpoint or arrow-create gesture is
	 *  currently hovering this box and would attach on release.
	 *  Renders a "drop here" outline as visual feedback. */
	dropTarget: boolean;
	/** Pointer-down on the box body. App.tsx uses this to drive
	 *  click-to-select and drag-to-move. */
	onPointerDown: (id: string, e: React.PointerEvent) => void;
	/** Pointer-down on a resize handle. App.tsx drives the gesture. */
	onResizeHandleDown: (
		id: string,
		handle: ResizeHandle,
		e: React.PointerEvent,
	) => void;
	/** Live text update — called on every keystroke. */
	onChangeText: (id: string, text: string) => void;
	/** Exit edit mode (blur or Esc). */
	onExitEdit: () => void;
}

const DEFAULT_FILL = "transparent";
const DEFAULT_STROKE = "#ffffff";
const DEFAULT_STROKE_WIDTH = 2;
const DEFAULT_RADIUS = 6;

const HANDLES: ResizeHandle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

/** One `@ocif/rect` node rendered as an absolute-positioned div in
 *  canvas space. Uses `border` + `box-sizing: border-box` so the
 *  rendered outer width / height match the doc-stored OCIF size
 *  exactly (the border draws *inside* the box, not outside). The
 *  border-radius is a render-time treatment — not part of the
 *  `@ocif/rect` spec — so it stays in CSS rather than the doc. */
export function OcifBox({
	id,
	node,
	selected,
	editing,
	interactive,
	dropTarget,
	onPointerDown,
	onResizeHandleDown,
	onChangeText,
	onExitEdit,
}: OcifBoxProps) {
	const handlePointerDown = useCallback(
		(e: React.PointerEvent) => {
			// While editing, swallow pointerdown on the body so the
			// drag-to-move handler doesn't kick in mid-edit.
			if (editing) {
				e.stopPropagation();
				return;
			}
			// In box / arrow draw mode, let the event bubble to the
			// canvas-level handler so a drag-to-create gesture can
			// start on top of an existing box (drawing an arrow from
			// box A to box B is the canonical use case).
			if (!interactive) return;
			e.stopPropagation();
			onPointerDown(id, e);
		},
		[id, onPointerDown, editing, interactive],
	);
	const fill = node.rect?.fill_color ?? DEFAULT_FILL;
	const stroke = node.rect?.stroke_color ?? DEFAULT_STROKE;
	const strokeWidth = node.rect?.stroke_width ?? DEFAULT_STROKE_WIDTH;
	const className = dropTarget
		? s.dropTarget
		: selected
			? s.selected
			: s.box;
	return (
		<div
			data-testid="ocif-box"
			data-node-id={id}
			className={className}
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
		>
			<OcifTextLayer
				id={id}
				text={node.text ?? ""}
				editing={editing}
				textStyle={node.text_style}
				onChangeText={onChangeText}
				onExit={onExitEdit}
			/>
			{selected &&
				!editing &&
				interactive &&
				HANDLES.map((h) => (
					<div
						key={h}
						className={`${s.handle} ${s[`handle_${h}`]}`}
						data-resize-handle={h}
						onPointerDown={(e) => {
							e.stopPropagation();
							onResizeHandleDown(id, h, e);
						}}
					/>
				))}
		</div>
	);
}

