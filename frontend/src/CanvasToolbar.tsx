import s from "./CanvasToolbar.module.css";

export type CanvasTool = "pointer" | "box" | "arrow";

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
		</div>
	);
}

function ToolButton(props: {
	active: boolean;
	label: string;
	onClick: () => void;
	children: React.ReactNode;
}) {
	return (
		<button
			type="button"
			className={props.active ? s.buttonActive : s.button}
			data-tooltip={props.label}
			aria-label={props.label}
			aria-pressed={props.active}
			onClick={props.onClick}
		>
			{props.children}
		</button>
	);
}
