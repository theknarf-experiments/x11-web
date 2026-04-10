//! GTK / Qt application menu mirror.
//!
//! This module owns the per-sidecar DBus connection and the per-window
//! tasks that mirror an X11 application's menu structure into our wire
//! protocol's `MenuItem` tree.
//!
//! Two source protocols are planned:
//!
//! * **`org.gtk.Menus` + `org.gtk.Actions`** (this file) — used by GTK 3
//!   applications when the desktop shell sets `_GTK_SHELL_SHOWS_MENUBAR`
//!   on the root window. Apps export their menu under
//!   `_GTK_MENUBAR_OBJECT_PATH` and their actions under the
//!   `_GTK_APPLICATION_OBJECT_PATH` / `_GTK_WINDOW_OBJECT_PATH` paths.
//! * **`com.canonical.dbusmenu`** (PR 3) — Qt and Firefox; the consumer
//!   takes ownership of `com.canonical.AppMenu.Registrar` and apps call
//!   `RegisterWindow(xid, object_path)` against it.
//!
//! The frontend doesn't see either source protocol; it gets a uniform
//! tree of `x11_web_protocol::MenuItem`s and a per-window UUID.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use x11_web_protocol::{DisplayUpdate, MenuAction, MenuItem, MenuItemKind};
use zbus::zvariant::{OwnedValue, Value};

use crate::xserver::TaggedDisplayUpdate;

/// Object paths and bus name advertised by a GTK app's top-level window
/// via the `_GTK_*` X11 properties. Used by `MenuTracker::attach_gtk`.
#[derive(Debug, Clone, Default)]
pub struct GtkMenuPaths {
    /// `_GTK_UNIQUE_BUS_NAME` — the app's unique DBus name (e.g. `:1.42`).
    pub bus_name: String,
    /// `_GTK_MENUBAR_OBJECT_PATH` — the GMenuModel exported as the menubar.
    /// May be missing for headerbar-only GNOME apps; in that case
    /// `app_menu_path` is the only menu we have.
    pub menubar_path: Option<String>,
    /// `_GTK_APP_MENU_OBJECT_PATH` — the GMenuModel for the app menu
    /// (the hamburger menu in newer GTK).
    pub app_menu_path: Option<String>,
    /// `_GTK_APPLICATION_OBJECT_PATH` — `org.gtk.Actions` for `app.*`
    /// actions, shared across all the app's windows.
    pub app_actions_path: Option<String>,
    /// `_GTK_WINDOW_OBJECT_PATH` — `org.gtk.Actions` for `win.*`
    /// actions, scoped to this window.
    pub win_actions_path: Option<String>,
}

impl GtkMenuPaths {
    /// `true` if the paths name at least one menu source we can mirror.
    pub fn has_menu(&self) -> bool {
        !self.bus_name.is_empty()
            && (self.menubar_path.is_some() || self.app_menu_path.is_some())
    }
}

/// Owns the DBus connection and bookkeeping for every tracked window.
/// Cheap to clone — the inner state lives behind an `Arc<Mutex<...>>`.
#[derive(Clone)]
pub struct MenuTracker {
    inner: Arc<TrackerInner>,
}

struct TrackerInner {
    /// Connection to the per-sidecar session bus, or `None` if
    /// dbus-daemon failed to start (Phase 0 fallback).
    conn: Option<zbus::Connection>,
    /// Channel back to the X11Server's display update fan-in. Each
    /// outgoing update is tagged with the *X11 client* id that owns
    /// the window so the backend routes it correctly.
    update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
    /// Per-window tracker tasks, keyed by the window's UUID.
    windows: Mutex<HashMap<String, WindowEntry>>,
}

struct WindowEntry {
    cmd_tx: mpsc::UnboundedSender<TrackerCommand>,
    task: tokio::task::JoinHandle<()>,
}

/// Commands sent into a per-window tracker task.
enum TrackerCommand {
    /// Frontend clicked a menu item — invoke the action over DBus.
    Activate { action: MenuAction },
    /// Re-fetch the menu from scratch (e.g. on `Changed` signal).
    Refresh,
    /// Tear down the task and disconnect.
    Stop,
}

impl MenuTracker {
    /// Build a new MenuTracker. If `dbus_address` is `None` (the
    /// daemon never started), the tracker is a no-op — `attach_gtk`
    /// returns immediately and no menus are mirrored.
    pub async fn new(
        update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
        dbus_address: Option<String>,
    ) -> Self {
        let conn = match dbus_address {
            Some(addr) => match zbus::connection::Builder::address(addr.as_str()) {
                Ok(builder) => match builder.build().await {
                    Ok(c) => {
                        info!("MenuTracker connected to session bus");
                        Some(c)
                    }
                    Err(e) => {
                        warn!("MenuTracker DBus connect failed: {e}");
                        None
                    }
                },
                Err(e) => {
                    warn!("MenuTracker invalid DBus address: {e}");
                    None
                }
            },
            None => None,
        };

        Self {
            inner: Arc::new(TrackerInner {
                conn,
                update_tx,
                windows: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Whether DBus is available — i.e. whether `attach_gtk` will
    /// actually do anything.
    pub fn is_active(&self) -> bool {
        self.inner.conn.is_some()
    }

    /// Begin mirroring a GTK app's menu for the given top-level window.
    /// Replaces any existing tracker for the same window UUID.
    pub fn attach_gtk(
        &self,
        window_uuid: String,
        client_id: String,
        paths: GtkMenuPaths,
    ) {
        let conn = match self.inner.conn.clone() {
            Some(c) => c,
            None => {
                debug!("MenuTracker.attach_gtk: no DBus, ignoring");
                return;
            }
        };
        if !paths.has_menu() {
            debug!(
                "MenuTracker.attach_gtk: no menu paths for window {window_uuid}, ignoring"
            );
            return;
        }

        info!(
            "MenuTracker mirroring GTK menus for window {window_uuid} bus={} menubar={:?} app_menu={:?}",
            paths.bus_name, paths.menubar_path, paths.app_menu_path
        );

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let update_tx = self.inner.update_tx.clone();
        let task = tokio::spawn(run_gtk_window_task(
            conn,
            window_uuid.clone(),
            client_id,
            paths,
            cmd_rx,
            update_tx,
        ));

        let mut windows = self.inner.windows.lock().unwrap();
        if let Some(prev) = windows.insert(window_uuid, WindowEntry { cmd_tx, task }) {
            let _ = prev.cmd_tx.send(TrackerCommand::Stop);
            prev.task.abort();
        }
    }

    /// Stop tracking a window. Called when the X11 window is destroyed.
    pub fn detach(&self, window_uuid: &str) {
        let mut windows = self.inner.windows.lock().unwrap();
        if let Some(entry) = windows.remove(window_uuid) {
            let _ = entry.cmd_tx.send(TrackerCommand::Stop);
            entry.task.abort();
        }
    }

    /// Forward a menu activation from the frontend to the right tracker.
    pub fn activate(&self, window_uuid: &str, action: MenuAction) {
        let windows = self.inner.windows.lock().unwrap();
        if let Some(entry) = windows.get(window_uuid) {
            let _ = entry.cmd_tx.send(TrackerCommand::Activate { action });
        } else {
            warn!("MenuTracker.activate: no tracker for window {window_uuid}");
        }
    }
}

// =============================================================================
// Per-window async task — owns one connection's worth of menu state
// =============================================================================

/// Properties of a single GMenu item, indexed by string key. Maps
/// directly to the wire dict GTK sends: `a{sv}`.
type GtkItemProps = HashMap<String, OwnedValue>;
/// One menu group as returned by `org.gtk.Menus.Start`:
/// `(group_id, position_in_referencing_group, items)`.
type GtkMenuGroup = (u32, u32, Vec<GtkItemProps>);

#[zbus::proxy(
    interface = "org.gtk.Menus",
    default_service = "org.freedesktop.DBus",
    default_path = "/"
)]
trait GtkMenus {
    fn start(&self, groups: &[u32]) -> zbus::Result<Vec<GtkMenuGroup>>;
    fn end(&self, groups: &[u32]) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.gtk.Actions",
    default_service = "org.freedesktop.DBus",
    default_path = "/"
)]
trait GtkActions {
    fn list(&self) -> zbus::Result<Vec<String>>;
    /// `a{s(bgav)}` — map from action name to (enabled, param_signature, state).
    fn describe_all(
        &self,
    ) -> zbus::Result<HashMap<String, (bool, String, Vec<OwnedValue>)>>;
    fn activate(
        &self,
        action_name: &str,
        parameter: &[Value<'_>],
        platform_data: HashMap<&str, Value<'_>>,
    ) -> zbus::Result<()>;
}

async fn run_gtk_window_task(
    conn: zbus::Connection,
    window_uuid: String,
    client_id: String,
    paths: GtkMenuPaths,
    mut cmd_rx: mpsc::UnboundedReceiver<TrackerCommand>,
    update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
) {
    // Build a Menus proxy on whichever menu path the app advertised.
    // Prefer the menubar path; fall back to the app menu.
    let menu_path = match paths
        .menubar_path
        .as_deref()
        .or(paths.app_menu_path.as_deref())
    {
        Some(p) => p.to_string(),
        None => return,
    };

    let menus_proxy = match build_menus_proxy(&conn, &paths.bus_name, &menu_path).await {
        Ok(p) => p,
        Err(e) => {
            warn!("GtkMenus proxy build failed for {window_uuid}: {e}");
            return;
        }
    };

    let app_actions = if let Some(path) = &paths.app_actions_path {
        match build_actions_proxy(&conn, &paths.bus_name, path).await {
            Ok(p) => Some(p),
            Err(e) => {
                debug!("app actions proxy build failed: {e}");
                None
            }
        }
    } else {
        None
    };

    let win_actions = if let Some(path) = &paths.win_actions_path {
        match build_actions_proxy(&conn, &paths.bus_name, path).await {
            Ok(p) => Some(p),
            Err(e) => {
                debug!("win actions proxy build failed: {e}");
                None
            }
        }
    } else {
        None
    };

    // Initial fetch + push.
    if let Err(e) = fetch_and_publish(
        &menus_proxy,
        app_actions.as_ref(),
        win_actions.as_ref(),
        &window_uuid,
        &client_id,
        &update_tx,
    )
    .await
    {
        warn!("MenuTracker initial fetch failed for {window_uuid}: {e}");
    }

    // Command loop. We don't subscribe to the `Changed` signal yet —
    // for v1 we re-fetch on explicit Refresh and after each Activate
    // (since some apps mutate menu state in response to activation).
    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            TrackerCommand::Stop => break,
            TrackerCommand::Refresh => {
                let _ = fetch_and_publish(
                    &menus_proxy,
                    app_actions.as_ref(),
                    win_actions.as_ref(),
                    &window_uuid,
                    &client_id,
                    &update_tx,
                )
                .await;
            }
            TrackerCommand::Activate { action } => {
                if let Err(e) = dispatch_activation(
                    app_actions.as_ref(),
                    win_actions.as_ref(),
                    &action,
                )
                .await
                {
                    warn!(
                        "MenuTracker activate {action_name} failed: {e}",
                        action_name = action.name
                    );
                }
                // Re-fetch in case state changed (e.g. checkbox toggled).
                let _ = fetch_and_publish(
                    &menus_proxy,
                    app_actions.as_ref(),
                    win_actions.as_ref(),
                    &window_uuid,
                    &client_id,
                    &update_tx,
                )
                .await;
            }
        }
    }

    info!("MenuTracker task for {window_uuid} stopped");
}

async fn build_menus_proxy<'a>(
    conn: &zbus::Connection,
    bus_name: &str,
    object_path: &str,
) -> zbus::Result<GtkMenusProxy<'a>> {
    GtkMenusProxy::builder(conn)
        .destination(bus_name.to_string())?
        .path(object_path.to_string())?
        .build()
        .await
}

async fn build_actions_proxy<'a>(
    conn: &zbus::Connection,
    bus_name: &str,
    object_path: &str,
) -> zbus::Result<GtkActionsProxy<'a>> {
    GtkActionsProxy::builder(conn)
        .destination(bus_name.to_string())?
        .path(object_path.to_string())?
        .build()
        .await
}

/// Walk the GMenu group tree from group 0 outward, build a tree of
/// `MenuItem`s, and emit a `MenuStructure` update for the window.
async fn fetch_and_publish(
    menus: &GtkMenusProxy<'_>,
    app_actions: Option<&GtkActionsProxy<'_>>,
    win_actions: Option<&GtkActionsProxy<'_>>,
    window_uuid: &str,
    client_id: &str,
    update_tx: &mpsc::UnboundedSender<TaggedDisplayUpdate>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Pull *all* groups by chasing :section and :submenu references
    // breadth-first. Start with group 0, then add any group referenced
    // from items we just received.
    let mut groups: HashMap<u32, Vec<GtkItemProps>> = HashMap::new();
    let mut to_fetch: Vec<u32> = vec![0];
    let mut fetched: std::collections::HashSet<u32> = std::collections::HashSet::new();

    while !to_fetch.is_empty() {
        let batch: Vec<u32> = to_fetch
            .drain(..)
            .filter(|g| !fetched.contains(g))
            .collect();
        if batch.is_empty() {
            break;
        }
        for g in &batch {
            fetched.insert(*g);
        }
        let reply = menus.start(&batch).await?;
        for (group_id, _pos, items) in reply {
            // Discover linked groups in this group's items.
            for item in &items {
                if let Some(linked) = link_group(item, ":section") {
                    if !fetched.contains(&linked) {
                        to_fetch.push(linked);
                    }
                }
                if let Some(linked) = link_group(item, ":submenu") {
                    if !fetched.contains(&linked) {
                        to_fetch.push(linked);
                    }
                }
            }
            groups.insert(group_id, items);
        }
    }

    // Pull action descriptions so we know enabled state and current
    // check / radio values for each item.
    let mut actions: HashMap<String, (bool, String, Vec<OwnedValue>)> = HashMap::new();
    if let Some(p) = app_actions {
        match p.describe_all().await {
            Ok(map) => {
                for (k, v) in map {
                    actions.insert(format!("app.{k}"), v);
                }
            }
            Err(e) => debug!("app describe_all failed: {e}"),
        }
    }
    if let Some(p) = win_actions {
        match p.describe_all().await {
            Ok(map) => {
                for (k, v) in map {
                    actions.insert(format!("win.{k}"), v);
                }
            }
            Err(e) => debug!("win describe_all failed: {e}"),
        }
    }

    let menu = build_tree(&groups, 0, &actions);

    let _ = update_tx.send((
        client_id.to_string(),
        DisplayUpdate::MenuStructure {
            window_id: window_uuid.to_string(),
            menu,
        },
    ));
    Ok(())
}

/// Pull `(group, position)` out of an item's `:section` or `:submenu`
/// link entry. Returns just the group id — we always pull the linked
/// group in full and let `build_tree` walk it.
fn link_group(item: &GtkItemProps, key: &str) -> Option<u32> {
    let v = item.get(key)?;
    let value: &Value = v;
    // Expected: (uu) — a 2-tuple of u32. Some apps emit it as a Structure.
    if let Value::Structure(s) = value {
        let fields = s.fields();
        if let Some(Value::U32(g)) = fields.first() {
            return Some(*g);
        }
    }
    None
}

/// Recursively assemble a `MenuItem` tree from the fetched groups.
/// Sections (`:section` links) are flattened into the parent group with
/// a leading separator (matching how GTK shells render them); submenus
/// become nested children.
fn build_tree(
    groups: &HashMap<u32, Vec<GtkItemProps>>,
    group_id: u32,
    actions: &HashMap<String, (bool, String, Vec<OwnedValue>)>,
) -> Vec<MenuItem> {
    let mut out = Vec::new();
    let items = match groups.get(&group_id) {
        Some(items) => items,
        None => return out,
    };
    for (idx, raw) in items.iter().enumerate() {
        // Section link: flatten the linked group inline with a leading
        // separator (skip the separator if this is the very first item).
        if let Some(linked) = link_group(raw, ":section") {
            if !out.is_empty() {
                out.push(MenuItem {
                    id: format!("{group_id}.{idx}.sep"),
                    label: None,
                    kind: MenuItemKind::Separator,
                    enabled: true,
                    visible: true,
                    checked: None,
                    accelerator: None,
                    icon: None,
                    action: None,
                    children: Vec::new(),
                });
            }
            let mut nested = build_tree(groups, linked, actions);
            out.append(&mut nested);
            continue;
        }

        let id = format!("{group_id}.{idx}");
        let label = string_prop(raw, "label").map(strip_underscores);
        let action_name = string_prop(raw, "action");
        let target = raw.get("target").and_then(|v| owned_value_to_json(v));
        let accelerator = string_prop(raw, "accel").map(prettify_accel);
        let icon = string_prop(raw, "icon")
            .or_else(|| string_prop(raw, "verb-icon"));

        // Submenu? Pull child group recursively.
        if let Some(linked) = link_group(raw, ":submenu") {
            let children = build_tree(groups, linked, actions);
            out.push(MenuItem {
                id,
                label,
                kind: MenuItemKind::Submenu,
                enabled: true,
                visible: true,
                checked: None,
                accelerator,
                icon,
                action: None,
                children,
            });
            continue;
        }

        // Determine kind + state from the action description if any.
        let (kind, enabled, checked) = match action_name.as_deref() {
            Some(name) => match actions.get(name) {
                Some((enabled, _sig, state)) => {
                    let checked_state = state
                        .first()
                        .and_then(|v| {
                            let value: &Value = v;
                            match value {
                                Value::Bool(b) => Some(*b),
                                _ => None,
                            }
                        });
                    let kind = if checked_state.is_some() {
                        MenuItemKind::Checkbox
                    } else {
                        MenuItemKind::Normal
                    };
                    (kind, *enabled, checked_state)
                }
                None => (MenuItemKind::Normal, true, None),
            },
            None => (MenuItemKind::Normal, label.is_some(), None),
        };

        out.push(MenuItem {
            id,
            label,
            kind,
            enabled,
            visible: true,
            checked,
            accelerator,
            icon,
            action: action_name.map(|name| MenuAction { name, target }),
            children: Vec::new(),
        });
    }
    out
}

fn string_prop(item: &GtkItemProps, key: &str) -> Option<String> {
    let v = item.get(key)?;
    let value: &Value = v;
    match value {
        Value::Str(s) => Some(s.as_str().to_string()),
        _ => None,
    }
}

/// GTK labels use a single underscore to mark the mnemonic letter
/// (e.g. `"_File"`). Strip them before display — frontend doesn't show
/// mnemonics in v1.
fn strip_underscores(label: String) -> String {
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            // Double underscore is an escaped literal '_'.
            if chars.peek() == Some(&'_') {
                out.push('_');
                chars.next();
            }
            // Single underscore: skip (mnemonic marker).
            continue;
        }
        out.push(c);
    }
    out
}

/// Translate a GTK accelerator string ("<Primary>q", "<Shift><Ctrl>s")
/// into a friendlier display form. Frontend renders this verbatim; we
/// don't intercept the keystrokes ourselves yet.
fn prettify_accel(accel: String) -> String {
    let replacements = [
        ("<Primary>", "Ctrl+"),
        ("<Control>", "Ctrl+"),
        ("<Ctrl>", "Ctrl+"),
        ("<Shift>", "Shift+"),
        ("<Alt>", "Alt+"),
        ("<Meta>", "Meta+"),
        ("<Super>", "Super+"),
    ];
    let mut out = accel;
    for (from, to) in replacements {
        out = out.replace(from, to);
    }
    // Capitalise the trailing key letter for display.
    if let Some(last) = out.chars().last() {
        if last.is_ascii_lowercase() {
            let mut chars: Vec<char> = out.chars().collect();
            *chars.last_mut().unwrap() = last.to_ascii_uppercase();
            out = chars.into_iter().collect();
        }
    }
    out
}

/// Best-effort conversion of an OwnedValue to serde_json so we can
/// round-trip GVariant targets through the wire protocol. Only handles
/// the cases we actually see in GTK menu targets (mostly strings,
/// occasionally ints).
fn owned_value_to_json(v: &OwnedValue) -> Option<serde_json::Value> {
    let value: &Value = v;
    match value {
        Value::Str(s) => Some(serde_json::Value::String(s.as_str().to_string())),
        Value::Bool(b) => Some(serde_json::Value::Bool(*b)),
        Value::I32(n) => Some(serde_json::Value::Number((*n).into())),
        Value::U32(n) => Some(serde_json::Value::Number((*n).into())),
        Value::I64(n) => Some(serde_json::Value::Number((*n).into())),
        Value::F64(n) => serde_json::Number::from_f64(*n).map(serde_json::Value::Number),
        _ => None,
    }
}

/// Translate a frontend MenuAction back into a typed GVariant and
/// invoke `org.gtk.Actions.Activate` on the appropriate proxy.
async fn dispatch_activation(
    app_actions: Option<&GtkActionsProxy<'_>>,
    win_actions: Option<&GtkActionsProxy<'_>>,
    action: &MenuAction,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (proxy, name) = if let Some(stripped) = action.name.strip_prefix("app.") {
        (app_actions.ok_or("no app actions proxy")?, stripped)
    } else if let Some(stripped) = action.name.strip_prefix("win.") {
        (win_actions.ok_or("no win actions proxy")?, stripped)
    } else {
        return Err(format!("unsupported action namespace: {}", action.name).into());
    };

    // Build the parameter array. v1 only handles string targets — the
    // common case for menu items that take an enum-style payload.
    let parameters: Vec<Value<'_>> = match &action.target {
        Some(serde_json::Value::String(s)) => vec![Value::Str(s.as_str().into())],
        _ => Vec::new(),
    };
    let platform_data: HashMap<&str, Value<'_>> = HashMap::new();
    proxy.activate(name, &parameters, platform_data).await?;
    Ok(())
}
