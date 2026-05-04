import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import s from "./MarkdownArea.module.css";
import { buildSegments, parseDecorations } from "./parse";

interface Props {
	initial?: string;
	onChange?: (text: string) => void;
}

/** Inline-syntax-highlighted markdown editor on top of the W3C
 *  EditContext API. Chromium owns the editing surface — caret,
 *  click, drag-select, IME, clipboard, undo. The EC is the source
 *  of truth for the text; we read `ec.text` on every render and
 *  only own two glue points where Chromium leaves us hanging:
 *  pushing DOM-selection changes back into EC, and inserting `\n`
 *  on Enter (which doesn't fire `textupdate`). */
export function MarkdownArea({ initial = "", onChange }: Props) {
	const ref = useRef<HTMLDivElement>(null);
	const ecRef = useRef<EditContext | null>(null);
	const [, force] = useState({});

	const text = ecRef.current?.text ?? initial;
	const segments = buildSegments(text, parseDecorations(text));

	// After React commits the new text into the DOM, push the EC's
	// selection back into the DOM so the native caret paints at
	// the correct offset. Chromium updates the EC's text +
	// selection on `textupdate` but doesn't move the DOM
	// selection, so without this the caret stays at offset 0.
	useLayoutEffect(() => {
		const el = ref.current;
		const ec = ecRef.current;
		if (!el || !ec) return;
		let remaining = ec.selectionStart;
		const walker = el.ownerDocument.createTreeWalker(
			el,
			NodeFilter.SHOW_TEXT,
		);
		let node = walker.nextNode() as Text | null;
		while (node && remaining > node.data.length) {
			remaining -= node.data.length;
			node = walker.nextNode() as Text | null;
		}
		if (!node) return;
		const sel = el.ownerDocument.getSelection();
		if (!sel) return;
		const r = el.ownerDocument.createRange();
		r.setStart(node, remaining);
		r.collapse(true);
		sel.removeAllRanges();
		sel.addRange(r);
	}, [text]);

	useEffect(() => {
		const el = ref.current;
		if (!el || typeof EditContext === "undefined") return;
		const ec = new EditContext({ text: initial });
		el.editContext = ec;
		ecRef.current = ec;

		const onText = () => {
			// `flushSync` so the new text + DOM-selection sync land
			// synchronously before this event handler returns. Each
			// keystroke is its own DOM event, but Playwright (and
			// fast typists) can fire them tightly enough that React
			// would otherwise batch multiple textupdates into one
			// render — our useLayoutEffect would only re-position
			// the caret once at the end, leaving Chromium with a
			// stale DOM selection for the intermediate keystrokes.
			flushSync(() => force({}));
			onChange?.(ec.text);
		};

		// Mirror DOM selection (clicks, drags, arrows) into the EC.
		// Chromium paints the caret from DOM selection but doesn't
		// auto-sync that back to the EC. Skip boundary states
		// where the selection lands on the container element
		// rather than inside a text node — those happen
		// transiently after each re-render and would otherwise
		// reset EC.selection back to 0.
		const onSel = () => {
			const sel = el.ownerDocument.getSelection();
			if (!sel?.rangeCount) return;
			const r = sel.getRangeAt(0);
			if (!el.contains(r.startContainer)) return;
			if (r.startContainer.nodeType !== Node.TEXT_NODE) return;
			const probe = el.ownerDocument.createRange();
			probe.selectNodeContents(el);
			probe.setEnd(r.startContainer, r.startOffset);
			const start = probe.toString().length;
			probe.setEnd(r.endContainer, r.endOffset);
			const end = probe.toString().length;
			if (start !== ec.selectionStart || end !== ec.selectionEnd) {
				ec.updateSelection(start, end);
			}
		};

		// Chromium fires a regular `paste` DOM event for EC-attached
		// elements (clipboard data isn't routed through textupdate),
		// so we intercept it ourselves.
		const onPaste = (e: ClipboardEvent) => {
			const pasted = e.clipboardData?.getData("text/plain");
			if (!pasted) return;
			e.preventDefault();
			const start = ec.selectionStart;
			ec.updateText(start, ec.selectionEnd, pasted);
			ec.updateSelection(start + pasted.length, start + pasted.length);
			force({});
			onChange?.(ec.text);
		};

		ec.addEventListener("textupdate", onText);
		el.ownerDocument.addEventListener("selectionchange", onSel);
		el.addEventListener("paste", onPaste);
		return () => {
			ec.removeEventListener("textupdate", onText);
			el.ownerDocument.removeEventListener("selectionchange", onSel);
			el.removeEventListener("paste", onPaste);
			el.editContext = null;
			ecRef.current = null;
		};
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, []);

	return (
		<div
			ref={ref}
			className={s.area}
			tabIndex={0}
			onKeyDown={(e) => {
				// Chromium swallows Enter from `textupdate`; insert
				// `\n` by hand and keep the EC in sync.
				if (
					e.key !== "Enter" ||
					e.shiftKey ||
					e.ctrlKey ||
					e.metaKey
				)
					return;
				const ec = ecRef.current;
				if (!ec) return;
				e.preventDefault();
				const start = ec.selectionStart;
				ec.updateText(start, ec.selectionEnd, "\n");
				ec.updateSelection(start + 1, start + 1);
				force({});
				onChange?.(ec.text);
			}}
		>
			{segments.map((seg, i) => (
				<span
					// Stable key — same `<span>` across renders so
					// DOM selection survives keystrokes.
					key={i}
					className={
						seg.classes
							.map((c) => s[c])
							.filter(Boolean)
							.join(" ") || undefined
					}
				>
					{text.slice(seg.start, seg.end)}
				</span>
			))}
			{/* Trailing `<br>` so a `\n` at the end of the text
			 * renders an empty visible line. Without this, the
			 * caret on that empty line collapses to a zero-size
			 * rect at the end of the previous line. */}
			{text.endsWith("\n") && <br />}
		</div>
	);
}
