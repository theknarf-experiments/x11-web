import { useCallback, useEffect, useRef, useState } from "react";
import {
	AppContextMenu,
	type AppContextMenuItem,
} from "./AppContextMenu";
import s from "./GlobalMenuBar.module.css";
import type { MenuAction, MenuItem } from "./types";

interface GlobalMenuBarProps {
	/** Title of the currently focused window, or null when nothing is focused. */
	focusedTitle: string | null;
	/** Menu tree mirrored from the focused window's GTK / Qt app. */
	menu: MenuItem[] | null;
	/** Send an activation back to the focused window. */
	onActivate: (action: MenuAction) => void;
	/**
	 * Items shown when the user clicks the focused app's title in the
	 * bar. App.tsx builds these via `getAppContextMenuItems` so this
	 * stays in lock-step with the dock right-click menu — change one
	 * place, change both.
	 */
	appContextMenuItems: AppContextMenuItem[] | null;
}

/**
 * macOS-style global menu bar fixed to the top of the page. Renders
 * the focused window's app name plus its top-level menu items; clicks
 * open click-to-stay popovers, leaves dispatch via `onActivate`.
 *
 * Phase 0 / 1 — purely click-driven, no keyboard navigation, no
 * accelerator capture, no submenu hover delay. Good enough for the
 * core demo; the rest can land in a follow-up once it's proven out.
 */
export function GlobalMenuBar({
	focusedTitle,
	menu,
	onActivate,
	appContextMenuItems,
}: GlobalMenuBarProps) {
	const [openIndex, setOpenIndex] = useState<number | null>(null);
	const [appMenuAnchor, setAppMenuAnchor] = useState<{
		x: number;
		y: number;
	} | null>(null);
	const barRef = useRef<HTMLDivElement>(null);

	// Close any open menu when the focused window changes — otherwise
	// a stale dropdown would refer to the previous app's items.
	// biome-ignore lint/correctness/useExhaustiveDependencies: only react to focused-window swap
	useEffect(() => {
		setOpenIndex(null);
		setAppMenuAnchor(null);
	}, [focusedTitle]);

	// Click outside the bar (and any open menu) closes the dropdown.
	useEffect(() => {
		if (openIndex == null) return;
		function onPointerDown(e: PointerEvent) {
			if (!barRef.current?.contains(e.target as Node)) {
				setOpenIndex(null);
			}
		}
		document.addEventListener("pointerdown", onPointerDown);
		return () => document.removeEventListener("pointerdown", onPointerDown);
	}, [openIndex]);

	const fire = useCallback(
		(action: MenuAction | undefined) => {
			if (!action) return;
			onActivate(action);
			setOpenIndex(null);
		},
		[onActivate],
	);

	const topItems = menu ?? [];
	const titleClickable =
		!!appContextMenuItems && appContextMenuItems.length > 0;

	return (
		<div className={s.menuBar} data-testid="global-menu-bar" ref={barRef}>
			<button
				type="button"
				className={s.appTitle}
				data-testid="global-menu-bar-title"
				disabled={!titleClickable}
				onClick={(e) => {
					if (!titleClickable) return;
					const rect = e.currentTarget.getBoundingClientRect();
					setAppMenuAnchor({
						x: rect.left,
						y: rect.bottom + 4,
					});
				}}
			>
				{focusedTitle ?? "x11-web"}
			</button>
			{topItems.map((item, idx) => {
				const isOpen = openIndex === idx;
				const isSeparator = item.kind === "separator";
				if (isSeparator) return null;
				return (
					<div key={item.id} className={s.menuItemWrapper}>
						<button
							type="button"
							className={isOpen ? s.menuButtonOpen : s.menuButton}
							data-testid="global-menu-top-item"
							disabled={item.enabled === false}
							onClick={() => {
								// Top-level: always treat as a popover toggle.
								// If it has children, open them; otherwise just
								// activate directly (rare for top-level).
								if (item.children && item.children.length > 0) {
									setOpenIndex(isOpen ? null : idx);
								} else {
									fire(item.action);
								}
							}}
						>
							{item.label ?? "(unnamed)"}
						</button>
						{isOpen && item.children && item.children.length > 0 && (
							<MenuDropdown items={item.children} onActivate={fire} />
						)}
					</div>
				);
			})}
			{appMenuAnchor && appContextMenuItems && (
				<AppContextMenu
					items={appContextMenuItems}
					x={appMenuAnchor.x}
					y={appMenuAnchor.y}
					onClose={() => setAppMenuAnchor(null)}
				/>
			)}
		</div>
	);
}

interface MenuDropdownProps {
	items: MenuItem[];
	onActivate: (action: MenuAction | undefined) => void;
}

function MenuDropdown({ items, onActivate }: MenuDropdownProps) {
	return (
		<div
			className={s.dropdown}
			role="menu"
			data-testid="global-menu-dropdown"
		>
			{items.map((item) => {
				if (item.kind === "separator") {
					return <div key={item.id} className={s.separator} role="none" />;
				}
				const hasChildren = !!(item.children && item.children.length > 0);
				return (
					<div key={item.id} className={s.dropdownItemWrapper}>
						<MenuRow
							item={item}
							hasChildren={hasChildren}
							onActivate={onActivate}
						/>
					</div>
				);
			})}
		</div>
	);
}

interface MenuRowProps {
	item: MenuItem;
	hasChildren: boolean;
	onActivate: (action: MenuAction | undefined) => void;
}

function MenuRow({ item, hasChildren, onActivate }: MenuRowProps) {
	const [submenuOpen, setSubmenuOpen] = useState(false);
	const enabled = item.enabled !== false;
	const checked = item.checked === true;

	if (hasChildren) {
		return (
			<>
				<button
					type="button"
					className={s.dropdownItem}
					data-testid="global-menu-item"
					disabled={!enabled}
					onClick={() => setSubmenuOpen((v) => !v)}
				>
					<span className={s.itemLabel}>
						{checked ? "✓ " : ""}
						{item.label ?? ""}
					</span>
					<span className={s.submenuChevron}>▸</span>
				</button>
				{submenuOpen && item.children && (
					<div className={s.nestedDropdown}>
						<MenuDropdown items={item.children} onActivate={onActivate} />
					</div>
				)}
			</>
		);
	}

	return (
		<button
			type="button"
			className={s.dropdownItem}
			data-testid="global-menu-item"
			disabled={!enabled}
			onClick={() => onActivate(item.action)}
		>
			<span className={s.itemLabel}>
				{checked ? "✓ " : ""}
				{item.label ?? ""}
			</span>
			{item.accelerator && (
				<span className={s.accel}>{item.accelerator}</span>
			)}
		</button>
	);
}
