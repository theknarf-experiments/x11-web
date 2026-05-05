/// <reference path="./edit-context.d.ts" />
import { useEffect, useLayoutEffect, useRef } from "react";
import { flushSync } from "react-dom";
import s from "./MarkdownArea.module.css";
import { buildSegments, parseDecorations } from "./parse";

interface Props {
	value: string;
	onChange: (text: string) => void;
	/** Optional class merged onto the outer editable `<div>` so
	 *  consumers can override the default background / border /
	 *  size from outside. */
	className?: string;
}

/** Inline-syntax-highlighted markdown editor on top of the W3C
 *  EditContext API. Chromium owns the editing surface — caret,
 *  click, drag-select, IME, clipboard, undo. The component is
 *  controlled: `value` is the source of truth, `onChange` reports
 *  every keystroke / paste back to the host so it can persist
 *  the text (e.g. into Automerge) and re-render us with a new
 *  `value` when remote edits land.
 *
 *  We're on the hook for: parsing + rendering styled spans,
 *  forwarding `textupdate` into `onChange`, mirroring DOM selection
 *  back into EC for clicks, pushing EC.selection back into DOM so
 *  the native caret paints in the right spot, and inserting `\n`
 *  on Enter (which Chromium swallows from the textupdate path). */
export function MarkdownArea({ value, onChange, className }: Props) {
	const ref = useRef<HTMLDivElement>(null);
	const ecRef = useRef<EditContext | null>(null);
	const onChangeRef = useRef(onChange);
	onChangeRef.current = onChange;

	const segments = buildSegments(value, parseDecorations(value));

	// Sync `value` → `ec.text` whenever the controlled prop changes.
	// Local typing already keeps `ec.text` and `value` in lockstep
	// (Chromium updates `ec.text` synchronously on `textupdate`,
	// `flushSync` ensures the parent state catches up before the
	// next event), so this is a no-op for local edits. For *remote*
	// updates — the `value` prop arriving from a peer / Automerge
	// doc subscription — `ec.text` would otherwise stay stale, and
	// the next local keystroke would emit a `textupdate` whose
	// resulting text was diffed against the OLD value. The host
	// then writes that stale-derived text back into the CRDT and
	// the remote edit reverts.
	useLayoutEffect(() => {
		const ec = ecRef.current;
		if (!ec) return;
		if (ec.text === value) return;
		ec.updateText(0, ec.text.length, value);
		// Clamp selection — `updateText` may have shortened the
		// text out from under it.
		const start = Math.min(ec.selectionStart, value.length);
		const end = Math.min(ec.selectionEnd, value.length);
		ec.updateSelection(start, end);
	}, [value]);

	// Push EC selection back into the DOM after every render so
	// the native caret paints at the correct offset. Chromium
	// updates `ec.selectionStart` on `textupdate` but doesn't move
	// the DOM selection — without this the caret stays at offset 0.
	// Skip when this editor isn't focused: in a multi-editor page
	// (e.g., two collaborating tabs side-by-side) the unfocused
	// editor's effect would otherwise stomp the global DOM
	// selection out of the editor the user is actually typing in.
	useLayoutEffect(() => {
		const el = ref.current;
		const ec = ecRef.current;
		if (!el || !ec) return;
		if (el.ownerDocument.activeElement !== el) return;
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
	}, [value]);

	useEffect(() => {
		const el = ref.current;
		if (!el || typeof EditContext === "undefined") return;
		const ec = new EditContext({ text: value });
		el.editContext = ec;
		ecRef.current = ec;

		const onText = () => {
			// `flushSync` so the parent state update + caret sync
			// land synchronously before the next textupdate fires.
			// Tightly-packed keystrokes (Playwright, fast typists)
			// would otherwise batch into one render and the DOM
			// selection ends up one step behind.
			flushSync(() => onChangeRef.current(ec.text));
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
			onChangeRef.current(ec.text);
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
			className={className ? `${s.area} ${className}` : s.area}
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
				onChangeRef.current(ec.text);
			}}
		>
			{segments.map((seg, i) => (
				<span
					// Stable key — same `<span>` across renders so
					// the DOM selection survives keystrokes.
					key={i}
					className={
						seg.classes
							.map((c) => s[c])
							.filter(Boolean)
							.join(" ") || undefined
					}
				>
					{value.slice(seg.start, seg.end)}
				</span>
			))}
			{/* Trailing `<br>` so a `\n` at the end of the text
			 * renders an empty visible line. Without this, the
			 * caret on that empty line collapses to a zero-size
			 * rect at the end of the previous line. */}
			{value.endsWith("\n") && <br />}
		</div>
	);
}
