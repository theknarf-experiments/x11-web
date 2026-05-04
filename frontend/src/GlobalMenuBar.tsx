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
	/** Active workspace's name from the synced Automerge doc. Used as
	 * the bar title when no window is focused, and is editable inline. */
	workspaceName: string | null;
	/** Commit a new workspace name. Empty strings are ignored — the
	 * input falls back to the previous value. */
	onRenameWorkspace: (name: string) => void;
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
	workspaceName,
	onRenameWorkspace,
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
			{focusedTitle == null ? (
				<WorkspaceNameField
					name={workspaceName ?? "Workspace"}
					onCommit={onRenameWorkspace}
				/>
			) : (
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
					{focusedTitle}
				</button>
			)}
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
							onPointerEnter={() => {
								// macOS-style menu cruise: once any top-level
								// menu is open, hovering a sibling switches
								// to it without an extra click. We don't open
								// anything on hover when the bar is closed.
								if (
									openIndex !== null &&
									openIndex !== idx &&
									item.enabled !== false &&
									item.children &&
									item.children.length > 0
								) {
									setOpenIndex(idx);
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

interface WorkspaceNameFieldProps {
	name: string;
	onCommit: (next: string) => void;
}

/**
 * Inline-editable text field for the workspace name. Looks like
 * static text until clicked, then becomes a real text input. Commits
 * on blur or Enter; reverts on Escape. Discards empty submissions so
 * the user can't end up with a nameless workspace.
 */
function WorkspaceNameField({ name, onCommit }: WorkspaceNameFieldProps) {
	const [draft, setDraft] = useState(name);
	const [editing, setEditing] = useState(false);

	// Pull in remote changes (other tabs renaming the same workspace)
	// when we're not actively editing — overwriting an in-progress
	// edit would be bad UX.
	// biome-ignore lint/correctness/useExhaustiveDependencies: only reflect remote changes when idle
	useEffect(() => {
		if (!editing) setDraft(name);
	}, [name]);

	return (
		<input
			type="text"
			className={s.workspaceNameInput}
			data-testid="global-menu-bar-title"
			value={draft}
			onChange={(e) => setDraft(e.target.value)}
			onFocus={(e) => {
				setEditing(true);
				setDraft(name);
				e.currentTarget.select();
			}}
			onBlur={() => {
				const trimmed = draft.trim();
				if (trimmed && trimmed !== name) onCommit(trimmed);
				else setDraft(name);
				setEditing(false);
			}}
			onKeyDown={(e) => {
				if (e.key === "Enter") {
					e.currentTarget.blur();
				} else if (e.key === "Escape") {
					setDraft(name);
					e.currentTarget.blur();
				}
			}}
			spellCheck={false}
		/>
	);
}

interface MenuDropdownProps {
	items: MenuItem[];
	onActivate: (action: MenuAction | undefined) => void;
}

function MenuDropdown({ items, onActivate }: MenuDropdownProps) {
	// Lifted submenu state — only one of this dropdown's rows can
	// have its submenu open at a time. macOS-style hover cruise:
	// hovering a submenu-carrier opens it (closing any other);
	// hovering a leaf closes any open sibling submenu.
	const [openId, setOpenId] = useState<string | null>(null);

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
							isOpen={openId === item.id}
							onOpenSubmenu={() => setOpenId(item.id)}
							onCloseSiblings={() => setOpenId(null)}
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
	isOpen: boolean;
	onOpenSubmenu: () => void;
	onCloseSiblings: () => void;
	onActivate: (action: MenuAction | undefined) => void;
}

function MenuRow({
	item,
	hasChildren,
	isOpen,
	onOpenSubmenu,
	onCloseSiblings,
	onActivate,
}: MenuRowProps) {
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
					onPointerEnter={() => {
						if (enabled) onOpenSubmenu();
					}}
					onClick={() => {
						// Toggle on click for keyboard / explicit
						// open-close. Hover already opens; clicking
						// an already-open carrier collapses it.
						if (isOpen) onCloseSiblings();
						else onOpenSubmenu();
					}}
				>
					<span className={s.itemMark}>{checked ? "✓" : ""}</span>
					<span className={s.itemLabel}>{item.label ?? ""}</span>
					<span className={s.submenuChevron}>▸</span>
				</button>
				{isOpen && item.children && (
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
			onPointerEnter={onCloseSiblings}
			onClick={() => onActivate(item.action)}
		>
			<span className={s.itemMark}>{checked ? "✓" : ""}</span>
			<span className={s.itemLabel}>{item.label ?? ""}</span>
			<span className={s.accel}>{item.accelerator ?? ""}</span>
		</button>
	);
}
