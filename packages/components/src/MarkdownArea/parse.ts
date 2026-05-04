import type { Root } from "mdast";
import remarkParse from "remark-parse";
import { unified } from "unified";
import { visit } from "unist-util-visit";

/** A styled range over the source text. Multiple decorations can
 *  overlap (e.g. a `**bold**` span inside a `# Heading` line); the
 *  renderer takes the union of their classes per character. */
export interface Decoration {
	start: number;
	end: number;
	type:
		| "heading-1"
		| "heading-2"
		| "heading-3"
		| "heading-4"
		| "heading-5"
		| "heading-6"
		| "bold"
		| "italic"
		| "inline-code"
		| "code-block"
		| "blockquote"
		| "link"
		| "list-marker";
}

/** Singleton processor — building the unified pipeline is the
 *  expensive bit, so we do it once and reuse on every parse.
 *  Plugins can be added via `processor.use(...)` later if we want
 *  e.g. `remark-gfm` for tables / strikethrough. */
const processor = unified().use(remarkParse);

/** Parse `text` and emit a flat list of source-offset decorations.
 *  Each mdast node carries a `position` with `{ start.offset,
 *  end.offset }` covering the *full* source range — including the
 *  syntax markers (`#`, `**`, etc.) — which is exactly what we
 *  want: the markers stay visible in the editor, just styled to
 *  match the rendered construct. */
export function parseDecorations(text: string): Decoration[] {
	if (text.length === 0) return [];
	const tree = processor.parse(text) as Root;
	const out: Decoration[] = [];
	visit(tree, (node) => {
		if (!node.position) return;
		const start = node.position.start.offset;
		const end = node.position.end.offset;
		if (start === undefined || end === undefined) return;
		switch (node.type) {
			case "heading": {
				const depth = Math.min(6, Math.max(1, node.depth)) as
					| 1 | 2 | 3 | 4 | 5 | 6;
				out.push({ start, end, type: `heading-${depth}` });
				break;
			}
			case "strong":
				out.push({ start, end, type: "bold" });
				break;
			case "emphasis":
				out.push({ start, end, type: "italic" });
				break;
			case "inlineCode":
				out.push({ start, end, type: "inline-code" });
				break;
			case "code":
				out.push({ start, end, type: "code-block" });
				break;
			case "blockquote":
				out.push({ start, end, type: "blockquote" });
				break;
			case "link":
				out.push({ start, end, type: "link" });
				break;
		}
	});
	return out;
}

/** One contiguous run of source text sharing the same set of
 *  active decorations. Renderer maps each segment to a `<span>`
 *  with the union of class names. */
export interface Segment {
	start: number;
	end: number;
	classes: string[];
}

/** Sweep-line over the decoration boundaries: every time a
 *  decoration starts or ends, we cut a new segment. Within a
 *  segment, the active decoration set is stable, so a single span
 *  with the merged class list captures it. */
export function buildSegments(
	text: string,
	decorations: Decoration[],
): Segment[] {
	const points = new Set<number>([0, text.length]);
	for (const d of decorations) {
		points.add(d.start);
		points.add(d.end);
	}
	const cuts = [...points].sort((a, b) => a - b);
	const segments: Segment[] = [];
	for (let i = 0; i < cuts.length - 1; i++) {
		const start = cuts[i];
		const end = cuts[i + 1];
		if (end <= start) continue;
		const classes: string[] = [];
		for (const d of decorations) {
			if (d.start <= start && d.end >= end) {
				classes.push(d.type);
			}
		}
		segments.push({ start, end, classes });
	}
	if (segments.length === 0 && text.length > 0) {
		segments.push({ start: 0, end: text.length, classes: [] });
	}
	return segments;
}
