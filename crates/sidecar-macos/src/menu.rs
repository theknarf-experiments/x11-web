//! macOS application-menu mirroring via the Accessibility API.
//!
//! The X11 sidecar has its own MenuTracker that reads
//! `org.gtk.Menus` / `com.canonical.dbusmenu` and emits
//! `DisplayUpdate::MenuStructure` per top-level window. macOS apps
//! don't expose menus over DBus — they live in the per-app
//! `NSMenu` you'd see in the macOS menu bar, accessible from outside
//! the process only through AX (the same API the screen reader uses).
//!
//! Pipeline:
//!   1. `AXUIElementCreateApplication(pid)` → app root.
//!   2. `AXMenuBar` attribute → the menu bar element.
//!   3. Walk `AXChildren` recursively. Each `AXMenuBarItem`/
//!      `AXMenuItem` becomes a [`MenuItem`] in our protocol.
//!
//! We don't yet handle activation (`MenuActivate`) — emitting the
//! structure is enough for the global menu bar to render. The `id`
//! field encodes a path so future activation can re-walk.

use std::ptr::NonNull;
use std::time::Duration;

use objc2_application_services::AXUIElement;
use objc2_core_foundation::{CFArray, CFRetained, CFString, CFType};

use x11_web_protocol::{MenuAction, MenuItem, MenuItemKind};

use crate::ax::{application_root, attribute_bool, attribute_element, perform_action};

/// Cap on recursion depth — guards against pathological menus
/// (deeply nested submenus or AX cycles). Real menus rarely exceed
/// 5 levels.
const MAX_DEPTH: u32 = 8;

/// Read the running app at `pid`'s menu bar and translate into our
/// protocol. Empty `Vec` if AX denies us, the app has no menu bar
/// (background apps without a UI), or the bar has no items.
///
/// AX is RPC into the target process; each attribute read is a few
/// hundred microseconds. A typical app menu (~50 items including
/// submenus) takes 5-50 ms total. Don't call from a tight loop —
/// the caller should run this from `tokio::task::spawn_blocking`.
pub fn read_menu_bar(pid: i32) -> Vec<MenuItem> {
    let app = application_root(pid);
    let Some(menu_bar) = attribute_element(&app, "AXMenuBar") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk_children(&menu_bar, &mut out, &format!("p{pid}"), 0);
    out
}

/// Walk the `AXChildren` of `parent` and append a [`MenuItem`] for
/// each `AXMenuBarItem` / `AXMenuItem` child, recursing into
/// submenus.
fn walk_children(parent: &AXUIElement, out: &mut Vec<MenuItem>, id_prefix: &str, depth: u32) {
    if depth > MAX_DEPTH {
        return;
    }
    let Some(arr) = attribute_array(parent, "AXChildren") else {
        return;
    };
    let count = arr.count();
    for i in 0..count {
        // SAFETY: `value_at_index` returns a non-owning pointer into
        // the array; valid for the duration of `arr`'s borrow.
        let ptr = unsafe { arr.value_at_index(i) };
        if ptr.is_null() {
            continue;
        }
        let child: &AXUIElement = unsafe { &*(ptr as *const AXUIElement) };
        let id = format!("{id_prefix}/{i}");
        if let Some(item) = read_menu_item(child, id, depth + 1) {
            out.push(item);
        }
    }
}

/// Translate a single AX node into a [`MenuItem`]. Returns `None`
/// for nodes whose role isn't a menu item (e.g., the `AXMenu`
/// container that holds the items — we descend into it, but it
/// isn't itself an item).
fn read_menu_item(element: &AXUIElement, id: String, depth: u32) -> Option<MenuItem> {
    let role = attribute_string(element, "AXRole")?;
    if role != "AXMenuBarItem" && role != "AXMenuItem" {
        return None;
    }

    let title = attribute_string(element, "AXTitle").unwrap_or_default();
    let enabled = attribute_bool(element, "AXEnabled").unwrap_or(true);

    // `AXMenuItemMarkChar` carries the rendered mark — `"✓"` for a
    // checked checkbox, `"•"` for a selected radio, empty otherwise.
    // Treat its presence as "this item is currently checked," and
    // its character as a hint for which kind of toggle it is.
    // (Items that *could* be checkable but aren't currently checked
    // have no mark char and look identical to plain `Normal` items
    // in AX — there's no way to distinguish at rest. The class
    // gets corrected the next time the user checks one.)
    let mark = attribute_string(element, "AXMenuItemMarkChar").unwrap_or_default();
    let checked = if mark.is_empty() { None } else { Some(true) };

    // A menu item with a submenu has an `AXMenu` child holding the
    // submenu's items; one without is a leaf. Some apps mark
    // separators with empty title and no children.
    let children_arr = attribute_array(element, "AXChildren");
    let has_submenu = children_arr
        .as_ref()
        .map(|arr| array_contains_role(arr, "AXMenu"))
        .unwrap_or(false);

    let kind = if !has_submenu && title.is_empty() {
        MenuItemKind::Separator
    } else if has_submenu {
        MenuItemKind::Submenu
    } else if mark.contains('•') || mark.contains('◦') {
        MenuItemKind::Radio
    } else if !mark.is_empty() {
        MenuItemKind::Checkbox
    } else {
        MenuItemKind::Normal
    };

    let children = if has_submenu {
        let arr = children_arr.as_ref().unwrap();
        let mut subitems = Vec::new();
        let n = arr.count();
        for i in 0..n {
            let ptr = unsafe { arr.value_at_index(i) };
            if ptr.is_null() {
                continue;
            }
            let menu_or_other: &AXUIElement = unsafe { &*(ptr as *const AXUIElement) };
            if attribute_string(menu_or_other, "AXRole").as_deref() == Some("AXMenu") {
                walk_children(menu_or_other, &mut subitems, &id, depth);
            }
        }
        subitems
    } else {
        Vec::new()
    };

    let label = if matches!(kind, MenuItemKind::Separator) {
        None
    } else {
        Some(title)
    };

    // Activatable leaves get an action whose `name` carries the
    // path identifier (`p<pid>/i/j/k`). The macOS sidecar's
    // `MenuActivate` handler uses that to re-walk the AX tree and
    // dispatch `AXPress` on the leaf. Separators and submenu
    // parents are non-activatable.
    let action = match kind {
        MenuItemKind::Normal | MenuItemKind::Checkbox | MenuItemKind::Radio => Some(MenuAction {
            name: id.clone(),
            target: None,
        }),
        MenuItemKind::Submenu | MenuItemKind::Separator => None,
    };

    Some(MenuItem {
        id,
        label,
        kind,
        enabled,
        visible: true,
        checked,
        // `AXMenuItemCmdChar` + `AXMenuItemCmdModifiers` would let
        // us reconstruct the keyboard accelerator (e.g. "⌘Q"). Not
        // wired yet.
        accelerator: None,
        icon: None,
        action,
        children,
    })
}

/// Check whether any element in the array advertises the given role.
/// Used to detect whether a menu item has an `AXMenu` child (i.e.,
/// it's a submenu carrier) without retaining each child.
fn array_contains_role(arr: &CFArray, role: &str) -> bool {
    let n = arr.count();
    for i in 0..n {
        let ptr = unsafe { arr.value_at_index(i) };
        if ptr.is_null() {
            continue;
        }
        let elem: &AXUIElement = unsafe { &*(ptr as *const AXUIElement) };
        if attribute_string(elem, "AXRole").as_deref() == Some(role) {
            return true;
        }
    }
    false
}

/// `AXUIElementCopyAttributeValue` reading a `CFString`-valued
/// attribute. `None` for missing or non-string attributes.
fn attribute_string(element: &AXUIElement, attribute: &str) -> Option<String> {
    let cfkey = CFString::from_str(attribute);
    let mut raw: *const CFType = std::ptr::null();
    let result = unsafe { element.copy_attribute_value(&cfkey, NonNull::new_unchecked(&mut raw)) };
    if result.0 != 0 || raw.is_null() {
        return None;
    }
    let cf = unsafe { CFRetained::from_raw(NonNull::new_unchecked(raw as *mut CFType)) };
    cf.downcast::<CFString>().ok().map(|s| s.to_string())
}

/// `AXUIElementCopyAttributeValue` reading a `CFArray`-valued
/// attribute (notably `AXChildren`).
fn attribute_array(element: &AXUIElement, attribute: &str) -> Option<CFRetained<CFArray>> {
    let cfkey = CFString::from_str(attribute);
    let mut raw: *const CFType = std::ptr::null();
    let result = unsafe { element.copy_attribute_value(&cfkey, NonNull::new_unchecked(&mut raw)) };
    if result.0 != 0 || raw.is_null() {
        return None;
    }
    let cf = unsafe { CFRetained::from_raw(NonNull::new_unchecked(raw as *mut CFType)) };
    cf.downcast::<CFArray>().ok()
}

/// Convenience: read a menu bar with a hard wall-clock budget. AX
/// can occasionally hang if the target process is unresponsive;
/// returning empty after the budget keeps the sidecar's enumerator
/// loop responsive even when one app misbehaves.
#[allow(dead_code)]
pub fn read_menu_bar_with_timeout(pid: i32, budget: Duration) -> Vec<MenuItem> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(read_menu_bar(pid));
    });
    rx.recv_timeout(budget).unwrap_or_default()
}

/// Re-walk the AX tree at the path encoded by an action name (the
/// id we baked in during `read_menu_item`) and call `AXPress` on
/// the leaf. Used by the macOS sidecar's `MenuActivate` handler.
///
/// Path format: `"p<pid>/<i0>/<i1>/..."` — `<pid>` is the owning
/// process; each `<iN>` is a child index. We strip the prefix,
/// take the first index against `AXMenuBar.children` and each
/// subsequent index against the previous element's `AXMenu` child.
pub fn dispatch_action(action_name: &str) -> Result<(), String> {
    let (pid, indices) = parse_action_path(action_name)?;
    let app = application_root(pid);
    let menu_bar =
        attribute_element(&app, "AXMenuBar").ok_or_else(|| "no AXMenuBar".to_string())?;

    // Top-level item index addresses children of the menu bar
    // directly. Subsequent indices descend into the AXMenu child
    // each item carries.
    let mut cursor: CFRetained<AXUIElement> = menu_bar;
    for (depth, &idx) in indices.iter().enumerate() {
        // For depth > 0 we first need to step into the parent's
        // AXMenu child (the dropdown container), then index into
        // its children. The first hop (depth=0) addresses
        // AXMenuBar.children directly.
        if depth > 0 {
            cursor = step_into_axmenu(&cursor)
                .ok_or_else(|| format!("no AXMenu child at depth {depth}"))?;
        }
        cursor = nth_child(&cursor, idx)
            .ok_or_else(|| format!("missing child {idx} at depth {depth}"))?;
    }

    perform_action(&cursor, "AXPress").map_err(|e| format!("AXPress: {e}"))
}

fn parse_action_path(name: &str) -> Result<(i32, Vec<usize>), String> {
    let rest = name
        .strip_prefix('p')
        .ok_or_else(|| format!("not a macOS menu path: {name}"))?;
    let mut parts = rest.split('/');
    let pid: i32 = parts
        .next()
        .ok_or("missing pid")?
        .parse()
        .map_err(|e| format!("bad pid: {e}"))?;
    let mut indices = Vec::new();
    for p in parts {
        if p.is_empty() {
            continue;
        }
        indices.push(
            p.parse::<usize>()
                .map_err(|e| format!("bad path segment {p:?}: {e}"))?,
        );
    }
    if indices.is_empty() {
        return Err("path has no segments after pid".into());
    }
    Ok((pid, indices))
}

fn step_into_axmenu(parent: &AXUIElement) -> Option<CFRetained<AXUIElement>> {
    let arr = attribute_array(parent, "AXChildren")?;
    let n = arr.count();
    for i in 0..n {
        let ptr = unsafe { arr.value_at_index(i) };
        if ptr.is_null() {
            continue;
        }
        let child: &AXUIElement = unsafe { &*(ptr as *const AXUIElement) };
        if attribute_string(child, "AXRole").as_deref() == Some("AXMenu") {
            // Bump the retain count so the returned handle owns a
            // strong reference independent of `arr`'s lifetime.
            return Some(unsafe {
                CFRetained::retain(NonNull::new_unchecked(
                    child as *const _ as *mut AXUIElement,
                ))
            });
        }
    }
    None
}

fn nth_child(parent: &AXUIElement, idx: usize) -> Option<CFRetained<AXUIElement>> {
    let arr = attribute_array(parent, "AXChildren")?;
    if idx >= arr.count() as usize {
        return None;
    }
    let ptr = unsafe { arr.value_at_index(idx as isize) };
    if ptr.is_null() {
        return None;
    }
    let child: &AXUIElement = unsafe { &*(ptr as *const AXUIElement) };
    Some(unsafe {
        CFRetained::retain(NonNull::new_unchecked(
            child as *const _ as *mut AXUIElement,
        ))
    })
}
