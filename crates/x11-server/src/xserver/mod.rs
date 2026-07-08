//! X11 server implementation.
//!
//! This module implements a minimal but spec-compliant X11 server that
//! translates X11 protocol requests into DisplayUpdate messages for the frontend.

pub(crate) mod atoms;
pub(crate) mod client;
pub(crate) mod connection;
pub(crate) mod core;
mod dispatch;
pub(crate) mod event;
pub(crate) mod extensions;
pub(crate) mod grab;
pub(crate) mod handlers;
pub(crate) mod input;
pub(crate) mod reply;
pub(crate) mod request;
pub(crate) mod setup;
pub(crate) mod types;
pub(crate) mod window_tree;

// Re-exports used by main.rs and other crates
pub use types::{TaggedDisplayUpdate, WindowRouter};
// Re-exports used by render.rs and other sibling modules
pub(crate) use client::ClientState;
// Re-exports from input.rs for grab.rs
pub(crate) use input::{CROSSING_MODE_GRAB, CROSSING_MODE_UNGRAB};
// Re-exports from split-out modules
use dispatch::handle_request;
pub(crate) use window_tree::{ancestor_chain, compute_visibility, is_descendant_of};

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tokio::net::UnixListener;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::framebuffer::Framebuffer;
use x11rb_protocol::protocol::xproto::{BackingStore, WindowClass};

use self::atoms::AtomManager;
use self::core::*;
use self::types::*;

/// Minimal X11 server that accepts client connections and translates
/// X11 drawing operations into DisplayUpdate messages.
pub struct X11Server {
    display_number: u32,
    socket_path: PathBuf,
    /// Server-global startup time. Per the X11 spec, the `time` field
    /// on every event is "Server Time" — milliseconds since X server
    /// start, comparable across clients. If each connection had its
    /// own start instant, a fresh client's events would carry a tiny
    /// `time` value (~10s of ms) and apps using the field to dedupe
    /// or order events (e.g. Firefox vs. a focus_time of 30 000+)
    /// silently dropped them as stale.
    server_start: std::time::Instant,
    update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
    client_connected_tx: mpsc::UnboundedSender<(String, u32)>,
    window_router: WindowRouter,
    menu_tracker: crate::menus::MenuTracker,
    auth_cookie: [u8; 16],
    /// Clipboard event receiver (selection ownership changes, data responses).
    clipboard_notify_tx: mpsc::UnboundedSender<()>,
    /// Shared selections (exposed for clipboard bridge in main.rs).
    shared_selections: SharedSelections,
    /// Persistent clipboard data saved when a clipboard owner disconnects.
    persistent_clipboard: types::PersistentClipboard,
    /// Watch channel receiver for dynamic screen resize (RandR).
    screen_size_rx: types::ScreenSizeRx,
    /// Shared access control state (server-wide host-based access control).
    shared_access_control: types::SharedAccessControl,
    /// Shared SECURITY authorization tokens (for cross-connection token validation).
    shared_security_tokens: types::SharedSecurityTokens,
    /// Extension registry — central source of truth for all X11 extensions.
    extension_registry: Arc<extensions::ExtensionRegistry>,
}

impl X11Server {
    pub fn new(
        display_number: u32,
        update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
        client_connected_tx: mpsc::UnboundedSender<(String, u32)>,
        window_router: WindowRouter,
        menu_tracker: crate::menus::MenuTracker,
        clipboard_notify_tx: mpsc::UnboundedSender<()>,
        screen_size_rx: types::ScreenSizeRx,
    ) -> Self {
        let socket_path = PathBuf::from(format!("/tmp/.X11-unix/X{display_number}"));

        // Generate a random MIT-MAGIC-COOKIE-1 auth cookie from /dev/urandom.
        let mut auth_cookie = [0u8; 16];
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            use std::io::Read;
            let _ = f.read_exact(&mut auth_cookie);
        }

        let shared_selections: SharedSelections = Arc::new(Mutex::new(HashMap::new()));
        let persistent_clipboard: types::PersistentClipboard = Arc::new(Mutex::new(HashMap::new()));

        Self {
            display_number,
            socket_path,
            server_start: std::time::Instant::now(),
            update_tx,
            client_connected_tx,
            window_router,
            menu_tracker,
            auth_cookie,
            clipboard_notify_tx,
            shared_selections,
            persistent_clipboard,
            screen_size_rx,
            shared_access_control: Arc::new(Mutex::new(types::AccessControlState::new())),
            shared_security_tokens: Arc::new(Mutex::new(HashMap::new())),
            extension_registry: Arc::new({
                let mut reg = extensions::ExtensionRegistry::new();
                // Kill switch for bisecting app breakage caused by
                // partially implemented extensions: comma-separated
                // wire names, e.g. "XInputExtension,XKEYBOARD".
                if let Ok(list) = std::env::var("X11WEB_DISABLE_EXTENSIONS") {
                    reg.disable_by_names(&list);
                }
                reg
            }),
        }
    }

    /// Write an `.Xauthority` file containing the MIT-MAGIC-COOKIE-1 entry
    /// so that X11 clients that require auth (e.g. xterm, many toolkit apps)
    /// can authenticate against this server.
    pub fn write_xauthority(&self) -> io::Result<PathBuf> {
        let xauth_path = PathBuf::from("/tmp/.x11-web-Xauthority");
        let hostname = std::fs::read_to_string("/etc/hostname")
            .unwrap_or_else(|_| "localhost".to_string())
            .trim()
            .to_string();
        let display_num_str = self.display_number.to_string();
        let auth_name = b"MIT-MAGIC-COOKIE-1";

        let mut data = Vec::new();

        // Entry 1: FamilyLocal (256)
        data.extend_from_slice(&256u16.to_be_bytes());
        data.extend_from_slice(&(hostname.len() as u16).to_be_bytes());
        data.extend_from_slice(hostname.as_bytes());
        data.extend_from_slice(&(display_num_str.len() as u16).to_be_bytes());
        data.extend_from_slice(display_num_str.as_bytes());
        data.extend_from_slice(&(auth_name.len() as u16).to_be_bytes());
        data.extend_from_slice(auth_name);
        data.extend_from_slice(&(self.auth_cookie.len() as u16).to_be_bytes());
        data.extend_from_slice(&self.auth_cookie);

        // Entry 2: FamilyWild (65535) so any connection method works
        data.extend_from_slice(&65535u16.to_be_bytes());
        data.extend_from_slice(&0u16.to_be_bytes()); // empty address
        data.extend_from_slice(&(display_num_str.len() as u16).to_be_bytes());
        data.extend_from_slice(display_num_str.as_bytes());
        data.extend_from_slice(&(auth_name.len() as u16).to_be_bytes());
        data.extend_from_slice(auth_name);
        data.extend_from_slice(&(self.auth_cookie.len() as u16).to_be_bytes());
        data.extend_from_slice(&self.auth_cookie);

        std::fs::write(&xauth_path, &data)?;
        info!("Wrote Xauthority file to {}", xauth_path.display());
        Ok(xauth_path)
    }

    pub fn display_string(&self) -> String {
        format!(":{}", self.display_number)
    }

    pub async fn run(self) -> io::Result<()> {
        let dir = self.socket_path.parent().unwrap();
        tokio::fs::create_dir_all(dir).await.ok();
        tokio::fs::remove_file(&self.socket_path).await.ok();

        let listener = UnixListener::bind(&self.socket_path)?;
        info!(
            "X11 server listening on {} (DISPLAY={})",
            self.socket_path.display(),
            self.display_string()
        );

        static CONNECTION_COUNTER: AtomicU32 = AtomicU32::new(0);

        let shared_atoms: Arc<Mutex<AtomManager>> = Arc::new(Mutex::new(AtomManager::new()));
        let shared_windows: SharedWindows = Arc::new(Mutex::new(HashMap::new()));
        let shared_keymap: SharedKeymap = Arc::new(Mutex::new(HashMap::new()));
        let shared_wm_state: SharedWmState = Arc::new(Mutex::new(WmState {
            client_id: None,
            event_tx: None,
        }));
        let shared_event_router = EventRouter::new();
        let shared_selections = self.shared_selections.clone();
        let shared_pixmaps: types::SharedPixmaps = Arc::new(Mutex::new(HashMap::new()));
        let shared_pixmap_fbs: types::SharedPixmapFbs = Arc::new(Mutex::new(HashMap::new()));
        let shared_gcs: types::SharedGcs = Arc::new(Mutex::new(HashMap::new()));
        let client_registry: types::SharedClientRegistry = Arc::new(Mutex::new(Vec::new()));
        let shared_pointer: types::SharedPointer = Arc::new(Mutex::new((0, 0)));
        let shared_focus: types::SharedFocus = Arc::new(Mutex::new(ROOT_WINDOW));
        let event_broadcaster = types::EventBroadcaster::new();
        let server_grab: types::ServerGrabLock =
            Arc::new((Mutex::new(None), tokio::sync::Notify::new()));
        let shared_record_contexts: types::SharedRecordContexts =
            Arc::new(Mutex::new(HashMap::new()));

        // Pre-populate with root window
        {
            let mut windows = shared_windows.lock().unwrap();
            let mut root_properties: HashMap<u32, PropertyValue> = HashMap::new();

            let mut atoms_lock = shared_atoms.lock().unwrap();
            let atom_shows_menubar = atoms_lock.intern("_GTK_SHELL_SHOWS_MENUBAR", false);
            let atom_shows_app_menu = atoms_lock.intern("_GTK_SHELL_SHOWS_APP_MENU", false);

            let cardinal_one = 1u32.to_le_bytes().to_vec();
            for atom in [atom_shows_menubar, atom_shows_app_menu] {
                root_properties.insert(
                    atom,
                    PropertyValue {
                        prop_type: crate::xserver::atoms::predef::CARDINAL,
                        format: 32,
                        data: cardinal_one.clone(),
                    },
                );
            }

            // EWMH: _NET_SUPPORTED
            let net_supported_atom = atoms_lock.intern("_NET_SUPPORTED", false);
            let supported_atoms: Vec<u32> = vec![
                atoms_lock.intern("_NET_SUPPORTED", false),
                atoms_lock.intern("_NET_SUPPORTING_WM_CHECK", false),
                atoms_lock.intern("_NET_WM_NAME", false),
                atoms_lock.intern("_NET_WM_ICON", false),
                atoms_lock.intern("_NET_WM_ICON_NAME", false),
                atoms_lock.intern("_NET_WM_PID", false),
                // Window types
                atoms_lock.intern("_NET_WM_WINDOW_TYPE", false),
                atoms_lock.intern("_NET_WM_WINDOW_TYPE_NORMAL", false),
                atoms_lock.intern("_NET_WM_WINDOW_TYPE_DIALOG", false),
                atoms_lock.intern("_NET_WM_WINDOW_TYPE_SPLASH", false),
                atoms_lock.intern("_NET_WM_WINDOW_TYPE_TOOLBAR", false),
                atoms_lock.intern("_NET_WM_WINDOW_TYPE_UTILITY", false),
                atoms_lock.intern("_NET_WM_WINDOW_TYPE_NOTIFICATION", false),
                atoms_lock.intern("_NET_WM_WINDOW_TYPE_MENU", false),
                atoms_lock.intern("_NET_WM_WINDOW_TYPE_DROPDOWN_MENU", false),
                atoms_lock.intern("_NET_WM_WINDOW_TYPE_POPUP_MENU", false),
                atoms_lock.intern("_NET_WM_WINDOW_TYPE_TOOLTIP", false),
                atoms_lock.intern("_NET_WM_WINDOW_TYPE_DOCK", false),
                atoms_lock.intern("_NET_WM_WINDOW_TYPE_DESKTOP", false),
                // Window state atoms
                atoms_lock.intern("_NET_WM_STATE", false),
                atoms_lock.intern("_NET_WM_STATE_FULLSCREEN", false),
                atoms_lock.intern("_NET_WM_STATE_MAXIMIZED_HORZ", false),
                atoms_lock.intern("_NET_WM_STATE_MAXIMIZED_VERT", false),
                atoms_lock.intern("_NET_WM_STATE_HIDDEN", false),
                atoms_lock.intern("_NET_WM_STATE_ABOVE", false),
                atoms_lock.intern("_NET_WM_STATE_BELOW", false),
                atoms_lock.intern("_NET_WM_STATE_DEMANDS_ATTENTION", false),
                atoms_lock.intern("_NET_WM_STATE_FOCUSED", false),
                // Allowed actions
                atoms_lock.intern("_NET_WM_ALLOWED_ACTIONS", false),
                atoms_lock.intern("_NET_WM_ACTION_MOVE", false),
                atoms_lock.intern("_NET_WM_ACTION_RESIZE", false),
                atoms_lock.intern("_NET_WM_ACTION_MINIMIZE", false),
                atoms_lock.intern("_NET_WM_ACTION_CLOSE", false),
                atoms_lock.intern("_NET_WM_ACTION_FULLSCREEN", false),
                atoms_lock.intern("_NET_WM_ACTION_MAXIMIZE_HORZ", false),
                atoms_lock.intern("_NET_WM_ACTION_MAXIMIZE_VERT", false),
                // Client lists and focus
                atoms_lock.intern("_NET_ACTIVE_WINDOW", false),
                atoms_lock.intern("_NET_CLIENT_LIST", false),
                atoms_lock.intern("_NET_CLIENT_LIST_STACKING", false),
                // Desktop / geometry
                atoms_lock.intern("_NET_NUMBER_OF_DESKTOPS", false),
                atoms_lock.intern("_NET_CURRENT_DESKTOP", false),
                atoms_lock.intern("_NET_DESKTOP_NAMES", false),
                atoms_lock.intern("_NET_WORKAREA", false),
                atoms_lock.intern("_NET_DESKTOP_GEOMETRY", false),
                atoms_lock.intern("_NET_DESKTOP_VIEWPORT", false),
                // Frame / decorations
                atoms_lock.intern("_NET_FRAME_EXTENTS", false),
                atoms_lock.intern("_NET_REQUEST_FRAME_EXTENTS", false),
                // Misc EWMH
                atoms_lock.intern("_NET_WM_USER_TIME", false),
                atoms_lock.intern("_NET_WM_STRUT", false),
                atoms_lock.intern("_NET_WM_STRUT_PARTIAL", false),
                atoms_lock.intern("_NET_WM_PING", false),
                atoms_lock.intern("_NET_WM_SYNC_REQUEST", false),
                atoms_lock.intern("_NET_CLOSE_WINDOW", false),
                atoms_lock.intern("_NET_WM_MOVERESIZE", false),
                // Additional window state atoms
                atoms_lock.intern("_NET_WM_STATE_MODAL", false),
                atoms_lock.intern("_NET_WM_STATE_STICKY", false),
                atoms_lock.intern("_NET_WM_STATE_SKIP_TASKBAR", false),
                atoms_lock.intern("_NET_WM_STATE_SKIP_PAGER", false),
                atoms_lock.intern("_NET_WM_STATE_SHADED", false),
                // EWMH window opacity
                atoms_lock.intern("_NET_WM_WINDOW_OPACITY", false),
                // Additional EWMH
                atoms_lock.intern("_NET_MOVERESIZE_WINDOW", false),
                atoms_lock.intern("_NET_RESTACK_WINDOW", false),
                atoms_lock.intern("_NET_WM_DESKTOP", false),
                atoms_lock.intern("_NET_WM_VISIBLE_NAME", false),
                atoms_lock.intern("_NET_WM_FULLSCREEN_MONITORS", false),
                atoms_lock.intern("_NET_STARTUP_ID", false),
                atoms_lock.intern("_NET_WM_ACTION_SHADE", false),
                atoms_lock.intern("_NET_WM_ACTION_STICK", false),
                atoms_lock.intern("_NET_WM_ACTION_CHANGE_DESKTOP", false),
                // ICCCM
                atoms_lock.intern("WM_PROTOCOLS", false),
                atoms_lock.intern("WM_DELETE_WINDOW", false),
                atoms_lock.intern("WM_TAKE_FOCUS", false),
                atoms_lock.intern("WM_STATE", false),
                atoms_lock.intern("WM_CHANGE_STATE", false),
                atoms_lock.intern("WM_COLORMAP_WINDOWS", false),
            ];

            // Pre-intern XDND atoms so drag-and-drop works without extra round-trips
            atoms_lock.intern("XdndAware", false);
            atoms_lock.intern("XdndEnter", false);
            atoms_lock.intern("XdndLeave", false);
            atoms_lock.intern("XdndPosition", false);
            atoms_lock.intern("XdndDrop", false);
            atoms_lock.intern("XdndFinished", false);
            atoms_lock.intern("XdndStatus", false);
            atoms_lock.intern("XdndTypeList", false);
            atoms_lock.intern("XdndActionCopy", false);
            atoms_lock.intern("XdndActionMove", false);
            atoms_lock.intern("XdndActionLink", false);
            atoms_lock.intern("XdndActionAsk", false);
            atoms_lock.intern("XdndActionPrivate", false);
            atoms_lock.intern("XdndSelection", false);
            atoms_lock.intern("XdndProxy", false);
            let supported_data: Vec<u8> = supported_atoms
                .iter()
                .flat_map(|a| a.to_le_bytes())
                .collect();
            root_properties.insert(
                net_supported_atom,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::ATOM,
                    format: 32,
                    data: supported_data,
                },
            );

            // _NET_SUPPORTING_WM_CHECK: points to a dedicated child window (EWMH spec).
            // Both root and the check window carry this property; the check window
            // also carries _NET_WM_NAME = "x11-web".
            let net_supporting = atoms_lock.intern("_NET_SUPPORTING_WM_CHECK", false);
            root_properties.insert(
                net_supporting,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::WINDOW,
                    format: 32,
                    data: WM_CHECK_WINDOW.to_le_bytes().to_vec(),
                },
            );

            // _NET_WM_NAME on root
            let net_wm_name = atoms_lock.intern("_NET_WM_NAME", false);
            let utf8_string = atoms_lock.intern("UTF8_STRING", false);
            root_properties.insert(
                net_wm_name,
                PropertyValue {
                    prop_type: utf8_string,
                    format: 8,
                    data: b"x11-web".to_vec(),
                },
            );

            // _NET_NUMBER_OF_DESKTOPS = 1
            let net_num_desktops = atoms_lock.intern("_NET_NUMBER_OF_DESKTOPS", false);
            root_properties.insert(
                net_num_desktops,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::CARDINAL,
                    format: 32,
                    data: 1u32.to_le_bytes().to_vec(),
                },
            );

            // _NET_CURRENT_DESKTOP = 0
            let net_cur_desktop = atoms_lock.intern("_NET_CURRENT_DESKTOP", false);
            root_properties.insert(
                net_cur_desktop,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::CARDINAL,
                    format: 32,
                    data: 0u32.to_le_bytes().to_vec(),
                },
            );

            // _NET_DESKTOP_GEOMETRY
            let net_desktop_geom = atoms_lock.intern("_NET_DESKTOP_GEOMETRY", false);
            let mut geom_data = Vec::new();
            geom_data.extend_from_slice(&(SCREEN_WIDTH as u32).to_le_bytes());
            geom_data.extend_from_slice(&(SCREEN_HEIGHT as u32).to_le_bytes());
            root_properties.insert(
                net_desktop_geom,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::CARDINAL,
                    format: 32,
                    data: geom_data,
                },
            );

            // _NET_DESKTOP_VIEWPORT = (0, 0)
            let net_desktop_vp = atoms_lock.intern("_NET_DESKTOP_VIEWPORT", false);
            root_properties.insert(
                net_desktop_vp,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::CARDINAL,
                    format: 32,
                    data: vec![0; 8],
                },
            );

            // _NET_WORKAREA = (0, 0, SCREEN_WIDTH, SCREEN_HEIGHT)
            let net_workarea = atoms_lock.intern("_NET_WORKAREA", false);
            let mut workarea = Vec::new();
            workarea.extend_from_slice(&0u32.to_le_bytes());
            workarea.extend_from_slice(&0u32.to_le_bytes());
            workarea.extend_from_slice(&(SCREEN_WIDTH as u32).to_le_bytes());
            workarea.extend_from_slice(&(SCREEN_HEIGHT as u32).to_le_bytes());
            root_properties.insert(
                net_workarea,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::CARDINAL,
                    format: 32,
                    data: workarea,
                },
            );

            // _NET_CLIENT_LIST = empty
            let net_client_list = atoms_lock.intern("_NET_CLIENT_LIST", false);
            root_properties.insert(
                net_client_list,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::WINDOW,
                    format: 32,
                    data: Vec::new(),
                },
            );

            // _NET_CLIENT_LIST_STACKING = empty
            let net_client_list_stacking = atoms_lock.intern("_NET_CLIENT_LIST_STACKING", false);
            root_properties.insert(
                net_client_list_stacking,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::WINDOW,
                    format: 32,
                    data: Vec::new(),
                },
            );

            // _NET_ACTIVE_WINDOW = 0 (none)
            let net_active = atoms_lock.intern("_NET_ACTIVE_WINDOW", false);
            root_properties.insert(
                net_active,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::WINDOW,
                    format: 32,
                    data: 0u32.to_le_bytes().to_vec(),
                },
            );

            // _NET_DESKTOP_NAMES = "Desktop\0"
            let net_desktop_names = atoms_lock.intern("_NET_DESKTOP_NAMES", false);
            root_properties.insert(
                net_desktop_names,
                PropertyValue {
                    prop_type: utf8_string,
                    format: 8,
                    data: b"Desktop\0".to_vec(),
                },
            );

            // _XKB_RULES_NAMES — toolkit apps (GTK, Qt) read this to configure XKB.
            // Format: five null-terminated strings: rules, model, layout, variant, options
            let xkb_rules_atom = atoms_lock.intern("_XKB_RULES_NAMES", false);
            let xkb_rules_data = b"evdev\0pc105\0us\0\0\0".to_vec();
            root_properties.insert(
                xkb_rules_atom,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::STRING,
                    format: 8,
                    data: xkb_rules_data,
                },
            );

            // _NET_WM_CM_S0 — advertise compositing manager (our server composites)
            let net_wm_cm_atom = atoms_lock.intern("_NET_WM_CM_S0", false);
            root_properties.insert(
                net_wm_cm_atom,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::WINDOW,
                    format: 32,
                    data: ROOT_WINDOW.to_le_bytes().to_vec(),
                },
            );

            // RESOURCE_MANAGER — toolkit configuration string read by Xt/GTK/Qt
            // This provides sensible defaults for font DPI and related settings.
            let resource_mgr_atom = atoms_lock.intern("RESOURCE_MANAGER", false);
            let resource_mgr_data = b"Xft.dpi:\t96\nXft.antialias:\t1\nXft.hinting:\t1\nXft.hintstyle:\thintslight\nXft.rgba:\trgb\n".to_vec();
            root_properties.insert(
                resource_mgr_atom,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::STRING,
                    format: 8,
                    data: resource_mgr_data,
                },
            );

            // WM_CHECK_WINDOW properties: _NET_SUPPORTING_WM_CHECK points to itself,
            // _NET_WM_NAME = "x11-web" (required by EWMH spec).
            let mut wm_check_properties: HashMap<u32, PropertyValue> = HashMap::new();
            let net_supporting_wm_check = atoms_lock.intern("_NET_SUPPORTING_WM_CHECK", false);
            wm_check_properties.insert(
                net_supporting_wm_check,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::WINDOW,
                    format: 32,
                    data: WM_CHECK_WINDOW.to_le_bytes().to_vec(),
                },
            );
            let net_wm_name_atom = atoms_lock.intern("_NET_WM_NAME", false);
            let utf8_string_atom = atoms_lock.intern("UTF8_STRING", false);
            wm_check_properties.insert(
                net_wm_name_atom,
                PropertyValue {
                    prop_type: utf8_string_atom,
                    format: 8,
                    data: b"x11-web".to_vec(),
                },
            );

            drop(atoms_lock);

            windows.insert(
                ROOT_WINDOW,
                WindowState {
                    id: ROOT_WINDOW,
                    parent: 0,
                    x: 0,
                    y: 0,
                    width: SCREEN_WIDTH,
                    height: SCREEN_HEIGHT,
                    border_width: 0,
                    visual: ROOT_VISUAL,
                    depth: 24,
                    class: u16::from(WindowClass::INPUT_OUTPUT),
                    mapped: true,
                    event_mask: 0,
                    do_not_propagate_mask: 0,
                    background_pixel: 0x00000000,
                    background_pixmap: None,
                    border_pixel: 0,
                    border_pixmap: None,
                    override_redirect: false,
                    redirected: false,
                    framebuffer: Framebuffer::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32),
                    properties: root_properties,
                    owner_client_id: String::new(),
                    cursor: None,
                    children_order: vec![WM_CHECK_WINDOW],
                    retained_temporary: false,
                    bounding_shape: None,
                    clip_shape: None,
                    input_shape: None,
                    shape_select_clients: Vec::new(),
                    colormap: ROOT_COLORMAP,
                    backing_store: u32::from(BackingStore::NOT_USEFUL) as u8,
                    backing_planes: 0xFFFFFFFF,
                    backing_pixel: 0,
                    save_under: false,
                    visibility: 0,
                    backing_pixmap: None,
                    wm_hints_initial_state: None,
                    transient_for: None,
                    sync_request_counter: None,
                    sync_request_value: 0,
                    window_type: WindowType::Normal,
                    strut: None,
                    wm_hints_input: None,
                    wm_hints_window_group: None,
                    modal: false,
                    saved_geometry: None,
                },
            );

            // Insert the EWMH WM check window. It is a child of root, InputOnly
            // (class=2), never mapped or visible. Its sole purpose is to carry
            // _NET_SUPPORTING_WM_CHECK and _NET_WM_NAME per the EWMH spec.
            windows.insert(
                WM_CHECK_WINDOW,
                WindowState {
                    id: WM_CHECK_WINDOW,
                    parent: ROOT_WINDOW,
                    x: -1,
                    y: -1,
                    width: 1,
                    height: 1,
                    border_width: 0,
                    // InputOnly windows still need a real visual ID so
                    // client-side XVisualIDFromVisual lookups don't return
                    // NULL and crash GDK.
                    visual: ROOT_VISUAL,
                    depth: 0,
                    class: u16::from(WindowClass::INPUT_ONLY),
                    mapped: false,
                    event_mask: 0,
                    do_not_propagate_mask: 0,
                    background_pixel: 0,
                    background_pixmap: None,
                    border_pixel: 0,
                    border_pixmap: None,
                    override_redirect: true,
                    redirected: false,
                    framebuffer: Framebuffer::new(0, 0),
                    properties: wm_check_properties,
                    owner_client_id: String::new(),
                    cursor: None,
                    children_order: Vec::new(),
                    retained_temporary: false,
                    bounding_shape: None,
                    clip_shape: None,
                    input_shape: None,
                    shape_select_clients: Vec::new(),
                    colormap: ROOT_COLORMAP,
                    backing_store: u32::from(BackingStore::NOT_USEFUL) as u8,
                    backing_planes: 0xFFFFFFFF,
                    backing_pixel: 0,
                    save_under: false,
                    visibility: 0,
                    backing_pixmap: None,
                    wm_hints_initial_state: None,
                    transient_for: None,
                    sync_request_counter: None,
                    sync_request_value: 0,
                    window_type: WindowType::Normal,
                    strut: None,
                    wm_hints_input: None,
                    wm_hints_window_group: None,
                    modal: false,
                    saved_geometry: None,
                },
            );

            // Insert the XSETTINGS manager window. Like the WM check window it is
            // InputOnly, unmapped, and exists solely to own the _XSETTINGS_S0
            // selection and carry the _XSETTINGS_SETTINGS property.
            let mut xsettings_properties: HashMap<u32, PropertyValue> = HashMap::new();
            {
                let mut atoms_lock = shared_atoms.lock().unwrap();
                let xsettings_settings_atom = atoms_lock.intern("_XSETTINGS_SETTINGS", false);
                xsettings_properties.insert(
                    xsettings_settings_atom,
                    PropertyValue {
                        prop_type: xsettings_settings_atom,
                        format: 8,
                        data: setup::build_xsettings_data(),
                    },
                );
            }

            windows.insert(
                XSETTINGS_WINDOW,
                WindowState {
                    id: XSETTINGS_WINDOW,
                    parent: ROOT_WINDOW,
                    x: -1,
                    y: -1,
                    width: 1,
                    height: 1,
                    border_width: 0,
                    // See WM_CHECK_WINDOW comment — InputOnly still gets a
                    // real visual ID.
                    visual: ROOT_VISUAL,
                    depth: 0,
                    class: u16::from(WindowClass::INPUT_ONLY),
                    mapped: false,
                    event_mask: 0,
                    do_not_propagate_mask: 0,
                    background_pixel: 0,
                    background_pixmap: None,
                    border_pixel: 0,
                    border_pixmap: None,
                    override_redirect: true,
                    redirected: false,
                    framebuffer: Framebuffer::new(0, 0),
                    properties: xsettings_properties,
                    owner_client_id: String::new(),
                    cursor: None,
                    children_order: Vec::new(),
                    retained_temporary: false,
                    bounding_shape: None,
                    clip_shape: None,
                    input_shape: None,
                    shape_select_clients: Vec::new(),
                    colormap: ROOT_COLORMAP,
                    backing_store: u32::from(BackingStore::NOT_USEFUL) as u8,
                    backing_planes: 0xFFFFFFFF,
                    backing_pixel: 0,
                    save_under: false,
                    visibility: 0,
                    backing_pixmap: None,
                    wm_hints_initial_state: None,
                    transient_for: None,
                    sync_request_counter: None,
                    sync_request_value: 0,
                    window_type: WindowType::Normal,
                    strut: None,
                    wm_hints_input: None,
                    wm_hints_window_group: None,
                    modal: false,
                    saved_geometry: None,
                },
            );

            // Add XSETTINGS_WINDOW to root's children_order
            if let Some(root) = windows.get_mut(&ROOT_WINDOW) {
                root.children_order.push(XSETTINGS_WINDOW);
            }
        }

        // Set XSETTINGS_S0 selection owner to the XSETTINGS_WINDOW.
        // We use a dummy channel since the server itself owns this selection
        // and never needs to receive selection request events through it.
        {
            let (dummy_tx, _dummy_rx) = mpsc::unbounded_channel();
            if let Ok(mut sels) = shared_selections.lock() {
                sels.insert(
                    164,
                    SelectionEntry {
                        // 164 = _XSETTINGS_S0
                        owner: XSETTINGS_WINDOW,
                        event_tx: dummy_tx,
                        timestamp: 0,
                    },
                );
            }
        }

        // Register the server as the CLIPBOARD_MANAGER selection owner.
        // This advertises that we implement clipboard persistence per ICCCM:
        // when a CLIPBOARD owner disconnects, the server preserves the data
        // and takes over CLIPBOARD ownership.
        {
            let (mgr_tx, _mgr_rx) = mpsc::unbounded_channel();
            if let Ok(mut sels) = shared_selections.lock() {
                sels.insert(
                    179,
                    SelectionEntry {
                        // 179 = CLIPBOARD_MANAGER
                        owner: types::CLIPBOARD_MANAGER_WINDOW,
                        event_tx: mgr_tx,
                        timestamp: 0,
                    },
                );
            }
        }

        // Create the system tray manager window and own _NET_SYSTEM_TRAY_S0.
        // This advertises a system tray so tray-capable apps (nm-applet, etc.)
        // can discover it and dock their status icons.
        {
            let mut atoms_lock = shared_atoms.lock().unwrap();
            let tray_orientation_atom = atoms_lock.intern("_NET_SYSTEM_TRAY_ORIENTATION", false);
            let tray_visual_atom = atoms_lock.intern("_NET_SYSTEM_TRAY_VISUAL", false);
            drop(atoms_lock);

            let mut tray_properties: HashMap<u32, PropertyValue> = HashMap::new();
            // Orientation: 0 = horizontal
            tray_properties.insert(
                tray_orientation_atom,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::CARDINAL,
                    format: 32,
                    data: 0u32.to_le_bytes().to_vec(),
                },
            );
            // Visual: advertise the 32-bit ARGB visual (0x40) for alpha-aware tray icons
            tray_properties.insert(
                tray_visual_atom,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::VISUALID,
                    format: 32,
                    data: 0x40u32.to_le_bytes().to_vec(),
                },
            );

            let mut windows = shared_windows.lock().unwrap();
            windows.insert(
                types::SYSTEM_TRAY_WINDOW,
                WindowState {
                    id: types::SYSTEM_TRAY_WINDOW,
                    parent: ROOT_WINDOW,
                    x: -1,
                    y: -1,
                    width: 1,
                    height: 1,
                    border_width: 0,
                    // See WM_CHECK_WINDOW comment — InputOnly still gets a
                    // real visual ID.
                    visual: ROOT_VISUAL,
                    depth: 0,
                    class: u16::from(WindowClass::INPUT_ONLY),
                    mapped: false,
                    event_mask: 0,
                    do_not_propagate_mask: 0,
                    background_pixel: 0,
                    background_pixmap: None,
                    border_pixel: 0,
                    border_pixmap: None,
                    override_redirect: true,
                    redirected: false,
                    framebuffer: Framebuffer::new(0, 0),
                    properties: tray_properties,
                    owner_client_id: String::new(),
                    cursor: None,
                    children_order: Vec::new(),
                    retained_temporary: false,
                    bounding_shape: None,
                    clip_shape: None,
                    input_shape: None,
                    shape_select_clients: Vec::new(),
                    colormap: ROOT_COLORMAP,
                    backing_store: u32::from(BackingStore::NOT_USEFUL) as u8,
                    backing_planes: 0xFFFFFFFF,
                    backing_pixel: 0,
                    save_under: false,
                    visibility: 0,
                    backing_pixmap: None,
                    wm_hints_initial_state: None,
                    transient_for: None,
                    sync_request_counter: None,
                    sync_request_value: 0,
                    window_type: WindowType::Normal,
                    strut: None,
                    wm_hints_input: None,
                    wm_hints_window_group: None,
                    modal: false,
                    saved_geometry: None,
                },
            );
            if let Some(root) = windows.get_mut(&ROOT_WINDOW) {
                root.children_order.push(types::SYSTEM_TRAY_WINDOW);
            }
        }

        // Own _NET_SYSTEM_TRAY_S0 selection
        {
            let (tray_tx, _tray_rx) = mpsc::unbounded_channel();
            if let Ok(mut sels) = shared_selections.lock() {
                sels.insert(
                    186,
                    SelectionEntry {
                        // 186 = _NET_SYSTEM_TRAY_S0
                        owner: types::SYSTEM_TRAY_WINDOW,
                        event_tx: tray_tx,
                        timestamp: 0,
                    },
                );
            }
        }

        // Create the XIM (X Input Method) server window and set up the
        // XIM_SERVERS property on root so toolkit apps can discover our IM.
        {
            let mut atoms_lock = shared_atoms.lock().unwrap();
            let xim_server_name_atom = atoms_lock.intern("@server=x11web", false);
            let xim_servers_atom = atoms_lock.intern("XIM_SERVERS", false);
            let locales_atom = atoms_lock.intern("LOCALES", false);
            let transport_atom = atoms_lock.intern("TRANSPORT", false);
            drop(atoms_lock);

            // XIM window properties: LOCALES and TRANSPORT
            let mut xim_properties: HashMap<u32, PropertyValue> = HashMap::new();
            xim_properties.insert(
                locales_atom,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::STRING,
                    format: 8,
                    data: b"@locale=C,en,en_US,en_US.UTF-8,POSIX,\
zh_CN,zh_CN.UTF-8,zh_TW,zh_TW.UTF-8,\
ja_JP,ja_JP.UTF-8,ja_JP.eucJP,\
ko_KR,ko_KR.UTF-8,\
de_DE,de_DE.UTF-8,fr_FR,fr_FR.UTF-8,\
es_ES,es_ES.UTF-8,pt_BR,pt_BR.UTF-8,\
ru_RU,ru_RU.UTF-8,ar_SA,ar_SA.UTF-8,\
hi_IN,hi_IN.UTF-8,th_TH,th_TH.UTF-8,\
vi_VN,vi_VN.UTF-8"
                        .to_vec(),
                },
            );
            xim_properties.insert(
                transport_atom,
                PropertyValue {
                    prop_type: crate::xserver::atoms::predef::STRING,
                    format: 8,
                    data: b"@transport=X/".to_vec(),
                },
            );

            let mut windows = shared_windows.lock().unwrap();

            windows.insert(
                XIM_WINDOW,
                WindowState {
                    id: XIM_WINDOW,
                    parent: ROOT_WINDOW,
                    x: -1,
                    y: -1,
                    width: 1,
                    height: 1,
                    border_width: 0,
                    // See WM_CHECK_WINDOW comment — InputOnly still gets a
                    // real visual ID.
                    visual: ROOT_VISUAL,
                    depth: 0,
                    class: u16::from(WindowClass::INPUT_ONLY),
                    mapped: false,
                    event_mask: 0,
                    do_not_propagate_mask: 0,
                    background_pixel: 0,
                    background_pixmap: None,
                    border_pixel: 0,
                    border_pixmap: None,
                    override_redirect: true,
                    redirected: false,
                    framebuffer: Framebuffer::new(0, 0),
                    properties: xim_properties,
                    owner_client_id: String::new(),
                    cursor: None,
                    children_order: Vec::new(),
                    retained_temporary: false,
                    bounding_shape: None,
                    clip_shape: None,
                    input_shape: None,
                    shape_select_clients: Vec::new(),
                    colormap: ROOT_COLORMAP,
                    backing_store: u32::from(BackingStore::NOT_USEFUL) as u8,
                    backing_planes: 0xFFFFFFFF,
                    backing_pixel: 0,
                    save_under: false,
                    visibility: 0,
                    backing_pixmap: None,
                    wm_hints_initial_state: None,
                    transient_for: None,
                    sync_request_counter: None,
                    sync_request_value: 0,
                    window_type: WindowType::Normal,
                    strut: None,
                    wm_hints_input: None,
                    wm_hints_window_group: None,
                    modal: false,
                    saved_geometry: None,
                },
            );

            // Set XIM_SERVERS property on root: list of server name atoms.
            if let Some(root) = windows.get_mut(&ROOT_WINDOW) {
                root.children_order.push(XIM_WINDOW);
                root.properties.insert(
                    xim_servers_atom,
                    PropertyValue {
                        prop_type: crate::xserver::atoms::predef::ATOM,
                        format: 32,
                        data: xim_server_name_atom.to_le_bytes().to_vec(),
                    },
                );
            }
        }

        // Also listen on TCP port 6000+display_number for remote X11 clients.
        let tcp_port = 6000 + self.display_number as u16;
        let tcp_listener = match tokio::net::TcpListener::bind(("0.0.0.0", tcp_port)).await {
            Ok(l) => {
                info!("X11 TCP listener on port {tcp_port}");
                Some(l)
            }
            Err(e) => {
                warn!("Could not bind TCP port {tcp_port}: {e} (TCP transport disabled)");
                None
            }
        };

        // Macro to avoid code duplication between Unix and TCP accept paths.
        // Both call handle_client with a UnixStream. For TCP, we convert via
        // a Unix socketpair that bridges the TCP connection to the handler.
        macro_rules! spawn_unix_client {
            ($stream:expr, $peer_pid:expr) => {{
                let conn_index = CONNECTION_COUNTER.fetch_add(1, Ordering::Relaxed);
                let client_id = Uuid::new_v4().to_string();
                let peer_pid: u32 = $peer_pid;
                let update_tx = self.update_tx.clone();
                let (message_tx, message_rx) = mpsc::unbounded_channel();
                let _ = self.client_connected_tx.send((client_id.clone(), peer_pid));
                let cid = client_id.clone();
                let sw = shared_windows.clone();
                let skm = shared_keymap.clone();
                let wm = shared_wm_state.clone();
                let sa = shared_atoms.clone();
                let wr = self.window_router.clone();
                let mt = self.menu_tracker.clone();
                let er = shared_event_router.clone();
                let ss = shared_selections.clone();
                let cn = self.clipboard_notify_tx.clone();
                let sp = shared_pixmaps.clone();
                let spf = shared_pixmap_fbs.clone();
                let sg = shared_gcs.clone();
                let cr = client_registry.clone();
                let sptr = shared_pointer.clone();
                let sfoc = shared_focus.clone();
                let eb = event_broadcaster.clone();
                let sgl = server_grab.clone();
                let src = shared_record_contexts.clone();
                let pc = self.persistent_clipboard.clone();
                let ac = self.auth_cookie;
                let ssr = self.screen_size_rx.clone();
                let sacl = self.shared_access_control.clone();
                let sst = self.shared_security_tokens.clone();
                let exr = self.extension_registry.clone();
                let sst_start = self.server_start;
                let stream = $stream;
                tokio::spawn(async move {
                    if let Err(e) = connection::handle_client(
                        stream, client_id, update_tx, message_tx, message_rx, conn_index, peer_pid,
                        sw, skm, wm, sa, wr, mt, er, ss, cn, sp, spf, sg, cr, sptr, sfoc, eb, sgl, src, pc, ac,
                        ssr, sacl, sst, exr, sst_start,
                    )
                    .await
                    {
                        debug!("X11 client {cid} disconnected: {e}");
                    }
                });
            }};
        }

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, _addr)) => {
                            let peer_pid = stream.peer_cred().ok().and_then(|c| c.pid()).unwrap_or(0) as u32;
                            spawn_unix_client!(stream, peer_pid);
                        }
                        Err(e) => {
                            error!("Failed to accept Unix X11 connection: {e}");
                        }
                    }
                }
                result = async {
                    match &tcp_listener {
                        Some(l) => l.accept().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match result {
                        Ok((tcp_stream, addr)) => {
                            // Enforce host-based access control for TCP connections
                            {
                                let acl = self.shared_access_control.lock().unwrap();
                                if !acl.check_tcp_address(&addr) {
                                    warn!("TCP X11 connection from {addr} rejected by access control");
                                    drop(tcp_stream);
                                    continue;
                                }
                            }
                            info!("TCP X11 client from {addr}");
                            // Bridge TCP to Unix socketpair so handle_client can use
                            // the same Unix-socket-based code path (SCM_RIGHTS won't
                            // work over TCP, but recv_with_fds gracefully returns 0 fds).
                            let (local, remote) = tokio::net::UnixStream::pair()
                                .expect("socketpair");
                            tokio::spawn(async move {
                                let (mut tcp_r, mut tcp_w) = tcp_stream.into_split();
                                let (mut unix_r, mut unix_w) = remote.into_split();
                                let a = tokio::io::copy(&mut tcp_r, &mut unix_w);
                                let b = tokio::io::copy(&mut unix_r, &mut tcp_w);
                                let _ = tokio::try_join!(a, b);
                            });
                            spawn_unix_client!(local, 0u32);
                        }
                        Err(e) => {
                            error!("Failed to accept TCP X11 connection: {e}");
                        }
                    }
                }
            }
        }
    }
}

impl Drop for X11Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}
