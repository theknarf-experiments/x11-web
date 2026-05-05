import type { Meta, StoryObj } from "@storybook/react-vite";
import { Popover } from "./Popover.tsx";

const meta: Meta<typeof Popover> = {
	title: "Popover",
	component: Popover,
	parameters: {
		backgrounds: {
			options: {
				canvas: { name: "Canvas", value: "#1a1a1a" },
			},
		},
	},
	globals: {
		backgrounds: { value: "canvas" },
	},
	decorators: [
		(Story) => (
			<div style={{ padding: 80, color: "#ccc" }}>
				<Story />
			</div>
		),
	],
	argTypes: {
		side: {
			control: "select",
			options: ["top", "right", "bottom", "left"],
		},
	},
};

export default meta;
type Story = StoryObj<typeof Popover>;

const buttonStyle: React.CSSProperties = {
	padding: "6px 12px",
	background: "rgba(40,40,40,0.85)",
	border: "1px solid rgba(255,255,255,0.15)",
	borderRadius: 6,
	color: "#ddd",
	font: "12px system-ui, sans-serif",
	cursor: "pointer",
};

const panelStyle: React.CSSProperties = {
	padding: 8,
	background: "rgba(30,30,30,0.95)",
	border: "1px solid rgba(255,255,255,0.18)",
	borderRadius: 6,
	color: "#ddd",
	font: "12px system-ui, sans-serif",
};

export const Default: Story = {
	args: {
		side: "top",
		trigger: <button type="button" style={buttonStyle}>Open</button>,
		children: <div style={panelStyle}>Hello from the popover!</div>,
	},
};

/** A small menu that uses the `close` render-prop so picking an
 *  item dismisses the popover. */
export const WithCloseRenderProp: Story = {
	args: {
		side: "top",
		trigger: <button type="button" style={buttonStyle}>Pick a colour</button>,
	},
	render: (args) => (
		<Popover {...args}>
			{({ close }) => (
				<div
					style={{
						...panelStyle,
						display: "flex",
						flexDirection: "column",
						minWidth: 100,
					}}
				>
					{["Red", "Green", "Blue"].map((c) => (
						<button
							key={c}
							type="button"
							onClick={close}
							style={{
								background: "transparent",
								border: 0,
								padding: "4px 8px",
								color: "#ddd",
								cursor: "pointer",
								textAlign: "left",
							}}
						>
							{c}
						</button>
					))}
				</div>
			)}
		</Popover>
	),
};
