import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import s from "./AppContextMenu.module.css";

/**
 * One row in the per-app context menu (right-click on the dock,
 * click-on-title in the global menu bar — both render from the same
 * list).
 */
export interface AppContextMenuItem {
	label: string;
	onSelect: () => void;
	/** Renders in red — used for destructive actions like Close. */
	destructive?: boolean;
}

/**
 * Single source of truth for the per-application context menu.
 * Extend this to add new actions to *both* the dock right-click and
 * the global menu bar app-title click.
 */
export function getAppContextMenuItems(
	sidecarId: string,
	pid: number,
	callbacks: { onClose: (sidecarId: string, pid: number) => void },
): AppContextMenuItem[] {
	return [
		{
			label: "Close",
			destructive: true,
			onSelect: () => callbacks.onClose(sidecarId, pid),
		},
	];
}

interface AppContextMenuProps {
	items: AppContextMenuItem[];
	/** Anchor coordinates in viewport space. */
	x: number;
	y: number;
	onClose: () => void;
	/**
	 * If `true`, the menu's bottom edge is anchored to `y` (used by the
	 * dock so it opens *above* the icon). If `false` the top edge is
	 * anchored to `y` (used by the global menu bar so it opens below).
	 */
	openUpwards?: boolean;
}

export function AppContextMenu({
	items,
	x,
	y,
	onClose,
	openUpwards,
}: AppContextMenuProps) {
	const ref = useRef<HTMLDivElement>(null);

	// Click outside dismisses.
	useEffect(() => {
		function onPointerDown(e: PointerEvent) {
			if (!ref.current?.contains(e.target as Node)) {
				onClose();
			}
		}
		document.addEventListener("pointerdown", onPointerDown);
		return () => document.removeEventListener("pointerdown", onPointerDown);
	}, [onClose]);

	return createPortal(
		<div
			ref={ref}
			className={s.menu}
			data-testid="app-context-menu"
			style={{
				left: x,
				top: y,
				transform: openUpwards ? "translateY(-100%)" : undefined,
			}}
		>
			{items.map((item) => (
				<button
					key={item.label}
					type="button"
					className={
						item.destructive ? s.menuItemDestructive : s.menuItem
					}
					data-testid="app-context-menu-item"
					onClick={() => {
						item.onSelect();
						onClose();
					}}
				>
					{item.label}
				</button>
			))}
		</div>,
		document.body,
	);
}
