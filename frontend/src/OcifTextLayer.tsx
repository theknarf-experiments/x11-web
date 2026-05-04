import { useEffect, useMemo, useRef } from "react";
import s from "./OcifTextLayer.module.css";
import type { TextStyleExt } from "./workspaceSync";

interface TextLayerProps {
	id: string;
	text: string;
	editing: boolean;
	/** Optional `@ocif/textstyle` extension. When unset the
	 *  renderer falls back to OCIF defaults. */
	textStyle?: TextStyleExt;
	/** Called on every keystroke — the textarea is fully
	 *  controlled by the doc. */
	onChangeText: (id: string, text: string) => void;
	/** Exit edit mode (called on blur or Esc). */
	onExit: () => void;
}

/** OCIF v0.7 `@ocif/textstyle` defaults — spec values, except
 *  `color` (we render on a dark canvas, so default white reads
 *  better than the spec's "#000000"). */
const DEFAULT_FONT_SIZE_PX = 14;
const DEFAULT_FONT_FAMILY =
	'-apple-system, BlinkMacSystemFont, "SF Pro Text", system-ui, sans-serif';
const DEFAULT_COLOR = "#ffffff";
const DEFAULT_ALIGN: NonNullable<TextStyleExt["align"]> = "center";
const DEFAULT_BOLD = false;
const DEFAULT_ITALIC = false;

/** Inline text content for an OCIF node. Static span when not in
 *  edit mode; auto-focused textarea when editing. Edits go to the
 *  doc on every keystroke so sibling tabs see live updates — the
 *  textarea has no local draft state, `value` IS `node.text`.
 *
 *  Used by both `OcifBox` (text label inside a `@ocif/rect`) and
 *  `OcifText` (free-floating text-only node). The OCIF data model
 *  treats text as node content (separate from any shape ext) so
 *  the layer is intentionally shape-agnostic. */
export function OcifTextLayer({
	id,
	text,
	editing,
	textStyle,
	onChangeText,
	onExit,
}: TextLayerProps) {
	const taRef = useRef<HTMLTextAreaElement>(null);

	// Merge OCIF textstyle with defaults into a single CSS object
	// the static label and the textarea both consume.
	const style = useMemo<React.CSSProperties>(
		() => ({
			fontSize: `${textStyle?.font_size_px ?? DEFAULT_FONT_SIZE_PX}px`,
			fontFamily: textStyle?.font_family ?? DEFAULT_FONT_FAMILY,
			color: textStyle?.color ?? DEFAULT_COLOR,
			textAlign: textStyle?.align ?? DEFAULT_ALIGN,
			fontWeight: (textStyle?.bold ?? DEFAULT_BOLD) ? 600 : 400,
			fontStyle: (textStyle?.italic ?? DEFAULT_ITALIC) ? "italic" : "normal",
		}),
		[textStyle],
	);

	useEffect(() => {
		if (editing) {
			const ta = taRef.current;
			if (ta) {
				ta.focus();
				ta.select();
			}
		}
	}, [editing]);

	if (!editing) {
		if (!text) return null;
		return (
			<div className={s.text} style={style}>
				{text}
			</div>
		);
	}

	return (
		<textarea
			ref={taRef}
			className={s.editor}
			style={style}
			value={text}
			onChange={(e) => onChangeText(id, e.target.value)}
			onPointerDown={(e) => e.stopPropagation()}
			onClick={(e) => e.stopPropagation()}
			onKeyDown={(e) => {
				if (e.key === "Escape") {
					e.preventDefault();
					e.stopPropagation();
					onExit();
					return;
				}
				// Stop propagation so canvas-level shortcuts (Delete
				// to remove the selected node, etc.) don't fire
				// while typing.
				e.stopPropagation();
			}}
			onBlur={onExit}
			spellCheck={false}
		/>
	);
}
