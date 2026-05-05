import { MarkdownArea } from "@x11-web/components";
import { useCallback, useEffect, useRef } from "react";
import s from "./OcifMarkdown.module.css";
import type { ResizeHandle } from "./OcifBox";
import type { OcifNode } from "./workspaceSync";

interface OcifMarkdownProps {
	id: string;
	node: OcifNode;
	selected: boolean;
	/** When true (pointer mode) the note's drag handle (header
	 *  bar) intercepts pointerdown for select / drag-to-move.
	 *  False during draw modes so a gesture can start through the
	 *  note (drawing an arrow from a note, for example). */
	interactive: boolean;
	dropTarget: boolean;
	onPointerDown: (id: string, e: React.PointerEvent) => void;
	onResizeHandleDown: (
		id: string,
		handle: ResizeHandle,
		e: React.PointerEvent,
	) => void;
	onChangeText: (id: string, text: string) => void;
}

const HANDLES: ResizeHandle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

/** A free-floating markdown note. The `MarkdownArea` editor is
 *  always live — click-to-type, no separate edit mode. The
 *  header bar above doubles as the drag handle for moving the
 *  note around the canvas. Bounds are user-controlled via the
 *  corner / edge resize handles; the editor scrolls when content
 *  overflows rather than auto-fitting. */
export function OcifMarkdown({
	id,
	node,
	selected,
	interactive,
	dropTarget,
	onPointerDown,
	onResizeHandleDown,
	onChangeText,
}: OcifMarkdownProps) {
	const handleHeaderPointerDown = useCallback(
		(e: React.PointerEvent) => {
			if (!interactive) return;
			e.stopPropagation();
			onPointerDown(id, e);
		},
		[id, onPointerDown, interactive],
	);

	const handleChange = useCallback(
		(text: string) => onChangeText(id, text),
		[id, onChangeText],
	);

	// Native wheel handler — `InfiniteCanvas` attaches a
	// `passive: false` wheel listener on its viewport that always
	// calls `preventDefault`, so the browser's default scroll
	// never runs and React's synthetic `onWheel` can't reach it
	// (synthetic events live on the React root, not in the DOM
	// bubble chain). We intercept in the native bubble phase,
	// `stopPropagation` so the canvas pan handler never fires, and
	// drive the wrapper's `scrollTop` / `scrollLeft` ourselves.
	const editorWrapRef = useRef<HTMLDivElement>(null);
	useEffect(() => {
		const el = editorWrapRef.current;
		if (!el) return;
		const onWheel = (e: WheelEvent) => {
			e.stopPropagation();
			e.preventDefault();
			el.scrollTop += e.deltaY;
			el.scrollLeft += e.deltaX;
		};
		el.addEventListener("wheel", onWheel, { passive: false });
		return () => el.removeEventListener("wheel", onWheel);
	}, []);

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
			onKeyDown={(e) => {
				// Stop canvas-level shortcuts (Delete to remove the
				// selected node, etc.) from firing while the user
				// types in the editor. Bubble phase only — the
				// capture phase would kill `MarkdownArea`'s Enter /
				// arrow handlers before they ran.
				e.stopPropagation();
			}}
		>
			<div className={s.header} onPointerDown={handleHeaderPointerDown}>
				Markdown
			</div>
			<div ref={editorWrapRef} className={s.editorWrap}>
				<MarkdownArea
					className={s.editor}
					value={node.text ?? ""}
					onChange={handleChange}
				/>
			</div>
			{selected &&
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
