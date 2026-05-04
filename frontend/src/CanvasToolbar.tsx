import type { Hotkey } from "@tanstack/react-hotkeys";
import s from "./CanvasToolbar.module.css";
import { Tooltip } from "./Tooltip";

export type CanvasTool = "pointer" | "box" | "arrow" | "text" | "pen";

/** The hotkey for each tool, kept here so the toolbar tooltip and
 *  the global hotkey registration in App.tsx stay in lockstep.
 *  Values typed as `Hotkey` (the string-form, not `RawHotkey`
 *  objects) so `useHotkey` accepts them and the Tooltip's `string`
 *  prop is happy too. */
export const TOOL_HOTKEYS: Record<CanvasTool, Hotkey> = {
	pointer: "V",
	box: "B",
	arrow: "A",
	text: "T",
	pen: "P",
};

interface CanvasToolbarProps {
	tool: CanvasTool;
	onSelect: (tool: CanvasTool) => void;
}

/** Floating tool palette in the bottom-left of the viewport.
 *  v1: pointer / box / arrow. Future tools (text, freehand) slot
 *  in here as additional buttons. */
export function CanvasToolbar({ tool, onSelect }: CanvasToolbarProps) {
	return (
		<div className={s.toolbar} data-testid="canvas-toolbar">
			<ToolButton
				active={tool === "pointer"}
				label="Pointer"
				hotkey={TOOL_HOTKEYS.pointer}
				onClick={() => onSelect("pointer")}
			>
				<svg viewBox="0 0 16 16" width={16} height={16}>
					<path
						d="M3 2 L13 8 L8 9 L7 14 Z"
						fill="currentColor"
						stroke="currentColor"
						strokeWidth={1}
						strokeLinejoin="round"
					/>
				</svg>
			</ToolButton>
			<ToolButton
				active={tool === "box"}
				label="Box"
				hotkey={TOOL_HOTKEYS.box}
				onClick={() => onSelect("box")}
			>
				<svg viewBox="0 0 16 16" width={16} height={16}>
					<rect
						x={2}
						y={3}
						width={12}
						height={10}
						fill="none"
						stroke="currentColor"
						strokeWidth={1.5}
					/>
				</svg>
			</ToolButton>
			<ToolButton
				active={tool === "arrow"}
				label="Arrow"
				hotkey={TOOL_HOTKEYS.arrow}
				onClick={() => onSelect("arrow")}
			>
				<svg viewBox="0 0 16 16" width={16} height={16}>
					<path
						d="M2 13 L13 4"
						fill="none"
						stroke="currentColor"
						strokeWidth={1.5}
						strokeLinecap="round"
					/>
					<polygon
						points="13,4 9,4 13,8"
						fill="currentColor"
					/>
				</svg>
			</ToolButton>
			<ToolButton
				active={tool === "text"}
				label="Text"
				hotkey={TOOL_HOTKEYS.text}
				onClick={() => onSelect("text")}
			>
				<svg viewBox="0 0 16 16" width={16} height={16}>
					<path
						d="M3 3 H13 M8 3 V13"
						fill="none"
						stroke="currentColor"
						strokeWidth={1.75}
						strokeLinecap="round"
					/>
				</svg>
			</ToolButton>
			<ToolButton
				active={tool === "pen"}
				label="Pen"
				hotkey={TOOL_HOTKEYS.pen}
				onClick={() => onSelect("pen")}
			>
				<svg viewBox="0 0 16 16" width={16} height={16}>
					<path
						d="M2 14 C 4 11, 6 9, 9 7 C 11 5, 13 4, 14 3"
						fill="none"
						stroke="currentColor"
						strokeWidth={1.5}
						strokeLinecap="round"
					/>
					<circle cx={2} cy={14} r={1} fill="currentColor" />
				</svg>
			</ToolButton>
		</div>
	);
}

function ToolButton(props: {
	active: boolean;
	label: string;
	hotkey: string;
	onClick: () => void;
	children: React.ReactNode;
}) {
	return (
		<Tooltip label={props.label} hotkey={props.hotkey} side="right">
			<button
				type="button"
				className={props.active ? s.buttonActive : s.button}
				aria-label={`${props.label} (${props.hotkey})`}
				aria-pressed={props.active}
				aria-keyshortcuts={props.hotkey}
				onClick={props.onClick}
			>
				{props.children}
			</button>
		</Tooltip>
	);
}
