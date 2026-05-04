import { useCallback } from "react";
import s from "./OcifText.module.css";
import { OcifTextLayer } from "./OcifTextLayer";
import type { OcifNode } from "./workspaceSync";

interface OcifTextProps {
	id: string;
	node: OcifNode;
	selected: boolean;
	editing: boolean;
	/** When true (pointer mode) the node intercepts pointerdown for
	 *  select / drag-to-move. When false (any draw mode) the event
	 *  bubbles so a drag-create gesture can start through it. */
	interactive: boolean;
	/** True when an arrow endpoint or arrow-create gesture is
	 *  currently hovering this node. Renders a "drop here" outline. */
	dropTarget: boolean;
	onPointerDown: (id: string, e: React.PointerEvent) => void;
	/** Pointer-down on one of the four corner scale handles. App.tsx
	 *  drives the gesture; dragging scales `font_size_px`. */
	onScaleHandleDown: (id: string, e: React.PointerEvent) => void;
	onChangeText: (id: string, text: string) => void;
	onExitEdit: () => void;
}

const CORNERS = ["nw", "ne", "se", "sw"] as const;

/** A free-floating text node — an OCIF node whose only "shape" is
 *  its text content (no `@ocif/rect`, no `@ocif/arrow`). Renders
 *  a borderless container at the node's bounds with the shared
 *  `OcifTextLayer` inside. Selected nodes get a dashed outline so
 *  the bounds are visible without committing to box chrome. */
export function OcifText({
	id,
	node,
	selected,
	editing,
	interactive,
	dropTarget,
	onPointerDown,
	onScaleHandleDown,
	onChangeText,
	onExitEdit,
}: OcifTextProps) {
	const handlePointerDown = useCallback(
		(e: React.PointerEvent) => {
			if (editing) {
				e.stopPropagation();
				return;
			}
			if (!interactive) return;
			e.stopPropagation();
			onPointerDown(id, e);
		},
		[id, onPointerDown, editing, interactive],
	);
	const className = dropTarget
		? s.dropTarget
		: selected
			? s.selected
			: s.text;
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
				interactive &&
				!editing &&
				CORNERS.map((c) => (
					<div
						key={c}
						className={`${s.handle} ${s[`handle_${c}`]}`}
						data-scale-handle={c}
						onPointerDown={(e) => {
							e.stopPropagation();
							onScaleHandleDown(id, e);
						}}
					/>
				))}
		</div>
	);
}
