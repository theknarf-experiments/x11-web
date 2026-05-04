import { useCallback, useEffect, useRef } from "react";
import s from "./OcifMarkdown.module.css";
import type { ResizeHandle } from "./OcifBox";
import type { OcifNode } from "./workspaceSync";

interface OcifMarkdownProps {
	id: string;
	node: OcifNode;
	selected: boolean;
	editing: boolean;
	/** When true (pointer mode) the note intercepts pointerdown for
	 *  select / drag-to-move. False during draw modes so a gesture
	 *  can start through the note (drawing an arrow from a note,
	 *  for example). */
	interactive: boolean;
	dropTarget: boolean;
	onPointerDown: (id: string, e: React.PointerEvent) => void;
	onResizeHandleDown: (
		id: string,
		handle: ResizeHandle,
		e: React.PointerEvent,
	) => void;
	onChangeText: (id: string, text: string) => void;
	onExitEdit: () => void;
}

const HANDLES: ResizeHandle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

/** A free-floating markdown note. Resource carries `text/markdown`
 *  content; for now we render it as a raw `<textarea>` (no preview
 *  pass). Distinguished from plain-text nodes by a small "Markdown"
 *  header bar and a parchment-tinted background. Bounds are user-
 *  controlled via the corner / edge resize handles — the textarea
 *  scrolls when content overflows rather than auto-fitting. */
export function OcifMarkdown({
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
}: OcifMarkdownProps) {
	const taRef = useRef<HTMLTextAreaElement>(null);

	useEffect(() => {
		if (editing) {
			const ta = taRef.current;
			if (ta) {
				ta.focus();
				// Cursor at end (don't select all — markdown notes
				// often hold a lot of text and select-all-on-focus is
				// hostile).
				const len = ta.value.length;
				ta.setSelectionRange(len, len);
			}
		}
	}, [editing]);

	const handlePointerDown = useCallback(
		(e: React.PointerEvent) => {
			if (editing) {
				// Swallow pointerdown on the body so the drag-to-move
				// handler doesn't kick in mid-edit.
				e.stopPropagation();
				return;
			}
			if (!interactive) return;
			e.stopPropagation();
			onPointerDown(id, e);
		},
		[id, onPointerDown, editing, interactive],
	);

	const className = dropTarget ? s.dropTarget : selected ? s.selected : s.note;

	return (
		<div
			data-testid="ocif-markdown"
			data-node-id={id}
			data-ocif-attachable={id}
			className={className}
			style={{
				left: node.x,
				top: node.y,
				width: node.width,
				height: node.height,
				zIndex: Math.round(node.z),
			}}
			onPointerDown={handlePointerDown}
		>
			<div className={s.header}>Markdown</div>
			<textarea
				ref={taRef}
				className={s.editor}
				value={node.text ?? ""}
				readOnly={!editing}
				onChange={(e) => onChangeText(id, e.target.value)}
				onPointerDown={(e) => {
					if (editing) e.stopPropagation();
				}}
				onClick={(e) => {
					if (editing) e.stopPropagation();
				}}
				onKeyDown={(e) => {
					if (e.key === "Escape") {
						e.preventDefault();
						e.stopPropagation();
						onExitEdit();
						return;
					}
					// Stop propagation so canvas-level shortcuts
					// (Delete to remove the selected node, etc.) don't
					// fire while typing.
					e.stopPropagation();
				}}
				onBlur={onExitEdit}
				spellCheck={false}
				placeholder="# Heading&#10;&#10;Write markdown here…"
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
