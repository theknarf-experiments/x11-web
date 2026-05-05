import * as Automerge from "@automerge/automerge";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { useRef, useState } from "react";
import { userEvent } from "vitest/browser";
import { expect, waitFor, within } from "storybook/test";
import { MarkdownArea } from "./MarkdownArea.tsx";

// Index signature satisfies Automerge's `Record<string, unknown>`
// constraint on its generic doc type.
interface DocShape extends Record<string, unknown> {
	body: string;
}

/** Two `MarkdownArea`s wired to two Automerge docs that converge
 *  via `Automerge.merge` after every edit. The docs live in refs
 *  rather than `useState` because React's dev-mode strict checks
 *  freeze state objects, which corrupts Automerge's internal
 *  WASM-backed state. A separate `force()` triggers re-render
 *  when refs change. */
function Pair({ initial }: { initial: string }) {
	const aRef = useRef<Automerge.Doc<DocShape> | null>(null);
	const bRef = useRef<Automerge.Doc<DocShape> | null>(null);
	if (aRef.current === null || bRef.current === null) {
		const seed = Automerge.from<DocShape>({ body: initial });
		aRef.current = Automerge.clone(seed);
		bRef.current = Automerge.clone(seed);
	}
	const [, force] = useState({});

	const edit = (side: "a" | "b", next: string) => {
		const localRef = side === "a" ? aRef : bRef;
		const peerRef = side === "a" ? bRef : aRef;
		localRef.current = Automerge.change(localRef.current!, (d) => {
			Automerge.updateText(d, ["body"], next);
		});
		// Merge the new change into the peer so both editors
		// converge. Two tabs collaborating live would do this via
		// sync messages over WebRTC; here it's synchronous.
		peerRef.current = Automerge.merge(peerRef.current!, localRef.current);
		force({});
	};

	return (
		<>
			<div data-testid="host-a" style={{ width: 480 }}>
				<MarkdownArea
					value={aRef.current!.body ?? ""}
					onChange={(t) => edit("a", t)}
				/>
				<pre data-testid="value-a" style={{ whiteSpace: "pre-wrap" }}>
					{aRef.current!.body ?? ""}
				</pre>
			</div>
			<div data-testid="host-b" style={{ width: 480 }}>
				<MarkdownArea
					value={bRef.current!.body ?? ""}
					onChange={(t) => edit("b", t)}
				/>
				<pre data-testid="value-b" style={{ whiteSpace: "pre-wrap" }}>
					{bRef.current!.body ?? ""}
				</pre>
			</div>
		</>
	);
}

const meta: Meta<typeof Pair> = {
	title: "MarkdownArea/automerge",
	component: Pair,
	tags: ["!autodocs"],
};
export default meta;
type Story = StoryObj<typeof Pair>;

function findEditor(host: HTMLElement, which: "a" | "b"): HTMLElement {
	const wrap = within(host).getByTestId(`host-${which}`);
	const editor = wrap.querySelector(":scope > div");
	if (!(editor instanceof HTMLElement)) {
		throw new Error("editor not found");
	}
	return editor;
}

function getValue(host: HTMLElement, which: "a" | "b"): string {
	return within(host).getByTestId(`value-${which}`).textContent ?? "";
}

/** Diagnostic — drive Automerge directly (no React, no editor)
 *  to verify `Automerge.change` + `Automerge.updateText` produce
 *  the expected text on each call. */
export const DiagnoseAutomergeRaw: Story = {
	args: { initial: "" },
	play: async () => {
		let doc = Automerge.from<DocShape>({ body: "" });
		doc = Automerge.change(doc, (d) =>
			Automerge.updateText(d, ["body"], "a"),
		);
		const after_a = doc.body;
		doc = Automerge.change(doc, (d) =>
			Automerge.updateText(d, ["body"], "ab"),
		);
		const after_ab = doc.body;
		doc = Automerge.change(doc, (d) =>
			Automerge.updateText(d, ["body"], "abc"),
		);
		const after_abc = doc.body;
		expect({ after_a, after_ab, after_abc }).toEqual({
			after_a: "a",
			after_ab: "ab",
			after_abc: "abc",
		});
	},
};

/** Diagnostic — verify `Automerge.merge` produces the right text
 *  when one peer edits and we merge into a sibling clone. */
export const DiagnoseAutomergeMerge: Story = {
	args: { initial: "" },
	play: async () => {
		const seed = Automerge.from<DocShape>({ body: "" });
		let docA = Automerge.clone(seed);
		let docB = Automerge.clone(seed);
		docA = Automerge.change(docA, (d) =>
			Automerge.updateText(d, ["body"], "a"),
		);
		docB = Automerge.merge(docB, docA);
		const after_a = { a: docA.body, b: docB.body };
		docA = Automerge.change(docA, (d) =>
			Automerge.updateText(d, ["body"], "ab"),
		);
		docB = Automerge.merge(docB, docA);
		const after_ab = { a: docA.body, b: docB.body };
		expect({ after_a, after_ab }).toEqual({
			after_a: { a: "a", b: "a" },
			after_ab: { a: "ab", b: "ab" },
		});
	},
};

/** Diagnostic — capture per-keystroke `value` reflected back
 *  through the Automerge doc. */
export const DiagnoseAutomergeFlow: Story = {
	args: { initial: "" },
	play: async ({ canvasElement }) => {
		const editorA = findEditor(canvasElement, "a");
		await userEvent.click(editorA);
		const trace: string[] = [];
		for (const c of "abc") {
			await userEvent.keyboard(c);
			trace.push(getValue(canvasElement, "a"));
		}
		expect(trace).toEqual(["a", "ab", "abc"]);
	},
};

/** Diagnostic — same trace but bypass Automerge entirely. Pure
 *  React state. If this passes and the Automerge variant fails,
 *  the bug is in the Automerge bridge, not the component. */
function PlainStateHost({ initial }: { initial: string }) {
	const [text, setText] = useState(initial);
	return (
		<div data-testid="host-a" style={{ width: 480 }}>
			<MarkdownArea value={text} onChange={setText} />
			<pre data-testid="value-a">{text}</pre>
		</div>
	);
}

export const DiagnosePlainStateFlow: StoryObj<typeof PlainStateHost> = {
	render: (args) => <PlainStateHost {...args} />,
	args: { initial: "" },
	play: async ({ canvasElement }) => {
		const editorA = findEditor(canvasElement, "a");
		await userEvent.click(editorA);
		const trace: string[] = [];
		for (const c of "abc") {
			await userEvent.keyboard(c);
			trace.push(getValue(canvasElement, "a"));
		}
		expect(trace).toEqual(["a", "ab", "abc"]);
	},
};

/** Diagnostic — single editor + single Automerge doc, no merge,
 *  no Pair. Isolates whether the Automerge `change`/`updateText`
 *  bridge round-trips correctly through React render cycles. */
function SingleAutomergeHost({ initial }: { initial: string }) {
	const docRef = useRef<Automerge.Doc<DocShape> | null>(null);
	if (docRef.current === null) {
		docRef.current = Automerge.from<DocShape>({ body: initial });
	}
	const [, force] = useState({});
	const onChange = (next: string) => {
		docRef.current = Automerge.change(docRef.current!, (d) => {
			Automerge.updateText(d, ["body"], next);
		});
		force({});
	};
	return (
		<div data-testid="host-a" style={{ width: 480 }}>
			<MarkdownArea
				value={docRef.current.body ?? ""}
				onChange={onChange}
			/>
			<pre data-testid="value-a">{docRef.current.body ?? ""}</pre>
		</div>
	);
}

export const DiagnoseSingleAutomerge: StoryObj<typeof SingleAutomergeHost> = {
	render: (args) => <SingleAutomergeHost {...args} />,
	args: { initial: "" },
	play: async ({ canvasElement }) => {
		const editorA = findEditor(canvasElement, "a");
		await userEvent.click(editorA);
		const trace: string[] = [];
		for (const c of "abc") {
			await userEvent.keyboard(c);
			trace.push(getValue(canvasElement, "a"));
		}
		expect(trace).toEqual(["a", "ab", "abc"]);
	},
};

/** Typing in tab A flows into B's editor via Automerge merge. */
export const LocalEditPropagatesToPeer: Story = {
	args: { initial: "" },
	play: async ({ canvasElement }) => {
		const editorA = findEditor(canvasElement, "a");
		await userEvent.click(editorA);
		await userEvent.keyboard("hello from A");
		await waitFor(() =>
			expect(getValue(canvasElement, "a")).toBe("hello from A"),
		);
		await waitFor(() =>
			expect(getValue(canvasElement, "b")).toBe("hello from A"),
		);
	},
};

/** B's local typing appears in A's editor through the merge. */
export const RemoteEditAppearsInLocalEditor: Story = {
	args: { initial: "" },
	play: async ({ canvasElement }) => {
		const editorB = findEditor(canvasElement, "b");
		await userEvent.click(editorB);
		await userEvent.keyboard("typed in B");
		await waitFor(() =>
			expect(getValue(canvasElement, "a")).toBe("typed in B"),
		);
	},
};

/** Stale-`EditContext` regression — when a remote edit lands in
 *  the controlled `value` prop, the component's internal
 *  `EditContext.text` must update too. Otherwise the next local
 *  keystroke reports an `ec.text` derived from the *old* value,
 *  and `Automerge.updateText`'s diff against the up-to-date doc
 *  emits a destructive splice that deletes the remote text. */
export const TypingAfterRemoteEditPreservesBothPeers: Story = {
	args: { initial: "" },
	play: async ({ canvasElement }) => {
		const editorA = findEditor(canvasElement, "a");
		await userEvent.click(editorA);
		await userEvent.keyboard("hello");
		await waitFor(() =>
			expect(getValue(canvasElement, "b")).toBe("hello"),
		);

		// B types one char. With the bug, B's stale `ec.text=""`
		// makes Chromium emit a textupdate whose resulting text is
		// just that one char — `Automerge.updateText` then diffs
		// the doc's "hello" against "X" and erases "hello".
		const editorB = findEditor(canvasElement, "b");
		await userEvent.click(editorB);
		await userEvent.keyboard("X");

		await waitFor(() => {
			const a = getValue(canvasElement, "a");
			const b = getValue(canvasElement, "b");
			expect(a).toMatch(/hello/);
			expect(b).toMatch(/hello/);
		});
	},
};

/** Same root cause from the other direction — A types, B's edit
 *  arrives as a remote update, A then types again. A must still
 *  see B's text after typing. */
export const InterleavedEditsRetainBothPeersText: Story = {
	args: { initial: "" },
	play: async ({ canvasElement }) => {
		const editorA = findEditor(canvasElement, "a");
		await userEvent.click(editorA);
		await userEvent.keyboard("A1");
		await waitFor(() =>
			expect(getValue(canvasElement, "b")).toBe("A1"),
		);

		const editorB = findEditor(canvasElement, "b");
		await userEvent.click(editorB);
		await userEvent.keyboard("B2");
		await waitFor(() => {
			expect(getValue(canvasElement, "a")).toMatch(/A1/);
			expect(getValue(canvasElement, "a")).toMatch(/B2/);
		});

		await userEvent.click(editorA);
		await userEvent.keyboard("A3");
		await waitFor(() => {
			const a = getValue(canvasElement, "a");
			const b = getValue(canvasElement, "b");
			for (const fragment of ["A1", "B2", "A3"]) {
				expect(a).toMatch(new RegExp(fragment));
				expect(b).toMatch(new RegExp(fragment));
			}
		});
	},
};
