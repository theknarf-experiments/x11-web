import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";
import { MarkdownArea } from "./MarkdownArea.tsx";

/** Wrapper that mirrors `onChange` into local state for the
 *  "raw text" debug panel underneath the editor. */
function ControlledHost({ initial }: { initial: string }) {
	const [text, setText] = useState(initial);
	return (
		<div style={{ width: 480, fontFamily: "system-ui, sans-serif" }}>
			<MarkdownArea initial={initial} onChange={setText} />
			<details style={{ marginTop: 12, color: "#888", fontSize: 12 }}>
				<summary>raw text ({text.length} chars)</summary>
				<pre
					style={{
						background: "#111",
						color: "#ddd",
						padding: 8,
						borderRadius: 4,
						overflow: "auto",
						fontSize: 11,
						lineHeight: 1.4,
					}}
				>
					{text}
				</pre>
			</details>
		</div>
	);
}

const meta: Meta<typeof ControlledHost> = {
	title: "MarkdownArea",
	component: ControlledHost,
	parameters: {
		layout: "centered",
		backgrounds: { default: "dark" },
	},
};

export default meta;
type Story = StoryObj<typeof ControlledHost>;

export const Empty: Story = {
	args: {
		initial: "",
	},
};

export const Headings: Story = {
	args: {
		initial:
			"# Heading 1\n## Heading 2\n### Heading 3\n\nA regular paragraph follows the headings.",
	},
};

export const Mixed: Story = {
	args: {
		initial: [
			"# A note about CRDTs",
			"",
			"Markdown notes use **character-level** merging so two tabs",
			"editing the same note _converge_ on a coherent result.",
			"",
			"Inline `code` spans look like this; block code looks like:",
			"",
			"```",
			"automerge.updateText(doc, path, newText)",
			"```",
			"",
			"> Block quotes are also styled.",
		].join("\n"),
	},
};

export const LongDocument: Story = {
	args: {
		initial: [
			"# Day plan",
			"",
			"## Morning",
			"- Coffee",
			"- Stand-up",
			"- **Focus block**: pen-stroke perf",
			"",
			"## Afternoon",
			"- Code review (`OcifMarkdown`)",
			"- Browser test of *EditContext* on Safari 18",
			"",
			"## Evening",
			"Wrap up notes here. Lots of text to scroll through if the",
			"render layer hooks up correctly.",
		].join("\n"),
	},
};
