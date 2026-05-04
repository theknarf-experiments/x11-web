import type { Meta, StoryObj } from "@storybook/react-vite";
import { page, userEvent } from "vitest/browser";
import { expect, waitFor, within } from "storybook/test";
import { useState } from "react";
import { MarkdownArea } from "./MarkdownArea.tsx";

/** Test host that captures every value the component reports so
 *  play functions can assert against the latest text without
 *  re-querying the rendered DOM. */
function TestHost({ initial }: { initial: string }) {
	const [text, setText] = useState(initial);
	return (
		<div data-testid="host" style={{ width: 480 }}>
			<MarkdownArea initial={initial} onChange={setText} />
			<pre data-testid="value" style={{ whiteSpace: "pre-wrap" }}>
				{text}
			</pre>
		</div>
	);
}

const meta: Meta<typeof TestHost> = {
	title: "MarkdownArea/tests",
	component: TestHost,
	tags: ["!autodocs"],
};

export default meta;
type Story = StoryObj<typeof TestHost>;

/** Resolve the inner editable div — the one with the
 *  EditContext attached — from the rendered host. */
function findEditor(canvas: HTMLElement): HTMLElement {
	const host = within(canvas).getByTestId("host");
	const editor = host.querySelector(":scope > div");
	if (!(editor instanceof HTMLElement)) {
		throw new Error("MarkdownArea editor div not found");
	}
	return editor;
}

function getValue(canvas: HTMLElement): string {
	return within(canvas).getByTestId("value").textContent ?? "";
}

export const TypingAtEnd: Story = {
	args: { initial: "" },
	play: async ({ canvasElement }) => {
		const editor = findEditor(canvasElement);
		await userEvent.click(editor);
		await userEvent.keyboard("Hello");
		await waitFor(() => expect(getValue(canvasElement)).toBe("Hello"));
	},
};

export const ClickThenType: Story = {
	args: { initial: "abcdef" },
	play: async ({ canvasElement }) => {
		const editor = findEditor(canvasElement);
		// Set the DOM selection between `c` and `d` (offset 3) via
		// Range API. Click-coords are flaky for sub-character
		// targeting; setting the selection directly is the
		// deterministic way to position. The component's
		// `selectionchange` listener should forward the new offset
		// into the EditContext.
		await userEvent.click(editor);
		const textNode = findFirstTextNode(editor);
		const range = editor.ownerDocument.createRange();
		range.setStart(textNode, 3);
		range.setEnd(textNode, 3);
		const sel = editor.ownerDocument.getSelection();
		sel?.removeAllRanges();
		sel?.addRange(range);
		await userEvent.keyboard("X");
		await waitFor(() => expect(getValue(canvasElement)).toBe("abcXdef"));
	},
};

export const EnterInsertsNewline: Story = {
	args: { initial: "" },
	play: async ({ canvasElement }) => {
		const editor = findEditor(canvasElement);
		await userEvent.click(editor);
		await userEvent.keyboard("hello");
		await userEvent.keyboard("{Enter}");
		await userEvent.keyboard("world");
		await waitFor(() =>
			expect(getValue(canvasElement)).toBe("hello\nworld"),
		);
	},
};

export const HeadingRendersStyled: Story = {
	args: { initial: "# Hello" },
	play: async ({ canvasElement }) => {
		const editor = findEditor(canvasElement);
		// First (and only) span should carry the heading-1 class.
		const span = editor.querySelector("span");
		expect(span).not.toBeNull();
		expect(span?.className).toContain("heading-1");
	},
};

/** Reproduces the originally-reported regression: a real visual
 *  click on rendered text positions the visible caret correctly,
 *  but a subsequent keystroke could insert at offset 0 if the
 *  EditContext's `selectionStart` wasn't synced from the DOM
 *  selection. We click at the pixel position of offset 6 (between
 *  the space and `W`), then type — the inserted character should
 *  land at offset 6, not 0. */
export const RealClickPositionsCaret: Story = {
	args: { initial: "Hello World" },
	play: async ({ canvasElement }) => {
		const editor = findEditor(canvasElement);
		const textNode = findFirstTextNode(editor);
		const range = editor.ownerDocument.createRange();
		range.setStart(textNode, 6);
		range.setEnd(textNode, 6);
		const targetRect = range.getBoundingClientRect();
		const editorRect = editor.getBoundingClientRect();
		await page
			.elementLocator(editor)
			.click({
				position: {
					x: targetRect.x - editorRect.x,
					y: targetRect.y + targetRect.height / 2 - editorRect.y,
				},
			});
		await userEvent.keyboard("X");
		await waitFor(() => expect(getValue(canvasElement)).toBe("Hello XWorld"));
	},
};

/** This is the exact sequence the user reported as broken: type
 *  some text, then click in the middle and type again. Each
 *  keystroke triggers a React re-render that replaces the rendered
 *  spans, and the DOM selection can end up collapsed onto the
 *  container element itself (boundary state) rather than inside a
 *  text node. If our `selectionchange` listener treats that
 *  boundary state as "user moved caret to offset 0", subsequent
 *  typing inserts at the wrong place. */
export const TypeThenClickThenType: Story = {
	args: { initial: "" },
	play: async ({ canvasElement }) => {
		const editor = findEditor(canvasElement);
		await userEvent.click(editor);
		await userEvent.keyboard("Hello World");
		await waitFor(() => expect(getValue(canvasElement)).toBe("Hello World"));

		// Click between the space and `W` — pixel-precise so the
		// browser positions the DOM caret exactly there.
		const textNode = findFirstTextNode(editor);
		const range = editor.ownerDocument.createRange();
		range.setStart(textNode, 6);
		range.setEnd(textNode, 6);
		const targetRect = range.getBoundingClientRect();
		const editorRect = editor.getBoundingClientRect();
		await page
			.elementLocator(editor)
			.click({
				position: {
					x: targetRect.x - editorRect.x,
					y: targetRect.y + targetRect.height / 2 - editorRect.y,
				},
			});
		await userEvent.keyboard("X");
		await waitFor(() =>
			expect(getValue(canvasElement)).toBe("Hello XWorld"),
		);
	},
};

/** Verifies that the DOM selection (where the native caret is
 *  painted) matches `ec.selectionStart` (where the next keystroke
 *  will insert). When they diverge the user sees the caret in one
 *  spot but text inserts somewhere else. */
export const CaretAndInsertionAgree: Story = {
	args: { initial: "" },
	play: async ({ canvasElement }) => {
		const editor = findEditor(canvasElement);
		await userEvent.click(editor);
		await userEvent.keyboard("a");
		await waitFor(() => expect(getValue(canvasElement)).toBe("a"));
		await new Promise((r) => requestAnimationFrame(() => r(undefined)));
		await new Promise((r) => requestAnimationFrame(() => r(undefined)));
		const ec = (editor as HTMLDivElement & { editContext: EditContext })
			.editContext;
		// Diagnostic: log current state so we can see what's happening.
		const sel = editor.ownerDocument.getSelection();
		const r = sel?.rangeCount ? sel.getRangeAt(0) : null;
		let domOffset = -1;
		let containerKind = "";
		if (r) {
			const probe = editor.ownerDocument.createRange();
			probe.selectNodeContents(editor);
			probe.setEnd(r.startContainer, r.startOffset);
			domOffset = probe.toString().length;
			containerKind =
				r.startContainer.nodeType === Node.TEXT_NODE ? "text" : "elem";
		}
		expect({
			ecText: ec.text,
			ecSelectionStart: ec.selectionStart,
			domOffset,
			containerKind,
		}).toEqual({
			ecText: "a",
			ecSelectionStart: 1,
			domOffset: 1,
			containerKind: "text",
		});
	},
};

function findFirstTextNode(root: Node): Text {
	const walker = root.ownerDocument!.createTreeWalker(
		root,
		NodeFilter.SHOW_TEXT,
	);
	const node = walker.nextNode();
	if (!(node instanceof Text)) {
		throw new Error("no text node inside MarkdownArea");
	}
	return node;
}

