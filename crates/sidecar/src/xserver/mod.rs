//! X11 server implementation.
//!
//! This module implements a minimal but spec-compliant X11 server that
//! translates X11 protocol requests into DisplayUpdate messages for the frontend.

pub(crate) mod atoms;
pub(crate) mod client;
#[allow(dead_code)]
pub(crate) mod core;
pub(crate) mod connection;
#[allow(dead_code)]
pub(crate) mod grab;
pub(crate) mod handlers;
pub(crate) mod input;
pub(crate) mod setup;
pub(crate) mod types;

// Re-exports used by main.rs and other crates
pub use types::{TaggedDisplayUpdate, WindowRouter};
// Re-exports used by render.rs and other sibling modules
pub(crate) use client::ClientState;
// Re-exports from input.rs for grab.rs
pub(crate) use input::{
    build_single_crossing_event,
    CROSSING_MODE_GRAB, CROSSING_MODE_UNGRAB,
};


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

use self::atoms::AtomManager;
use self::core::*;
use self::types::*;

/// Minimal X11 server that accepts client connections and translates
/// X11 drawing operations into DisplayUpdate messages.
pub struct X11Server {
    display_number: u32,
    socket_path: PathBuf,
    update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
    client_connected_tx: mpsc::UnboundedSender<(String, u32)>,
    window_router: WindowRouter,
    menu_tracker: crate::menus::MenuTracker,
    auth_cookie: [u8; 16],
    /// Clipboard event receiver (selection ownership changes, data responses).
    clipboard_notify_tx: mpsc::UnboundedSender<ClipboardEvent>,
    /// Server-side clipboard data (browser → X11, set via SetClipboard).
    shared_clipboard: SharedClipboard,
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
}

impl X11Server {
    pub fn new(
        display_number: u32,
        update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
        client_connected_tx: mpsc::UnboundedSender<(String, u32)>,
        window_router: WindowRouter,
        menu_tracker: crate::menus::MenuTracker,
        clipboard_notify_tx: mpsc::UnboundedSender<ClipboardEvent>,
        shared_clipboard: SharedClipboard,
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
            update_tx,
            client_connected_tx,
            window_router,
            menu_tracker,
            auth_cookie,
            clipboard_notify_tx,
            shared_clipboard,
            shared_selections,
            persistent_clipboard,
            screen_size_rx,
            shared_access_control: Arc::new(Mutex::new(types::AccessControlState::new())),
            shared_security_tokens: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get a clone of the shared selections (for clipboard bridge in main.rs).
    pub fn shared_selections(&self) -> SharedSelections {
        self.shared_selections.clone()
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
        let event_broadcaster = types::EventBroadcaster::new();
        let server_grab: types::ServerGrabLock = Arc::new((tokio::sync::Mutex::new(None), tokio::sync::Notify::new()));
        let shared_record_contexts: types::SharedRecordContexts = Arc::new(Mutex::new(HashMap::new()));

        // Pre-populate with root window
        {
            let mut windows = shared_windows.lock().unwrap();
            let mut root_properties: HashMap<u32, PropertyValue> = HashMap::new();

            let mut atoms_lock = shared_atoms.lock().unwrap();
            let atom_shows_menubar =
                atoms_lock.intern("_GTK_SHELL_SHOWS_MENUBAR", false);
            let atom_shows_app_menu =
                atoms_lock.intern("_GTK_SHELL_SHOWS_APP_MENU", false);

            let cardinal_one = 1u32.to_le_bytes().to_vec();
            for atom in [atom_shows_menubar, atom_shows_app_menu] {
                root_properties.insert(
                    atom,
                    PropertyValue {
                        prop_type: 6,
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
            let supported_data: Vec<u8> = supported_atoms.iter().flat_map(|a| a.to_le_bytes()).collect();
            root_properties.insert(net_supported_atom, PropertyValue {
                prop_type: 4, // ATOM
                format: 32,
                data: supported_data,
            });

            // _NET_SUPPORTING_WM_CHECK: points to a dedicated child window (EWMH spec).
            // Both root and the check window carry this property; the check window
            // also carries _NET_WM_NAME = "x11-web".
            let net_supporting = atoms_lock.intern("_NET_SUPPORTING_WM_CHECK", false);
            root_properties.insert(net_supporting, PropertyValue {
                prop_type: 33, // WINDOW
                format: 32,
                data: WM_CHECK_WINDOW.to_le_bytes().to_vec(),
            });

            // _NET_WM_NAME on root
            let net_wm_name = atoms_lock.intern("_NET_WM_NAME", false);
            let utf8_string = atoms_lock.intern("UTF8_STRING", false);
            root_properties.insert(net_wm_name, PropertyValue {
                prop_type: utf8_string,
                format: 8,
                data: b"x11-web".to_vec(),
            });

            // _NET_NUMBER_OF_DESKTOPS = 1
            let net_num_desktops = atoms_lock.intern("_NET_NUMBER_OF_DESKTOPS", false);
            root_properties.insert(net_num_desktops, PropertyValue {
                prop_type: 6, // CARDINAL
                format: 32,
                data: 1u32.to_le_bytes().to_vec(),
            });

            // _NET_CURRENT_DESKTOP = 0
            let net_cur_desktop = atoms_lock.intern("_NET_CURRENT_DESKTOP", false);
            root_properties.insert(net_cur_desktop, PropertyValue {
                prop_type: 6,
                format: 32,
                data: 0u32.to_le_bytes().to_vec(),
            });

            // _NET_DESKTOP_GEOMETRY
            let net_desktop_geom = atoms_lock.intern("_NET_DESKTOP_GEOMETRY", false);
            let mut geom_data = Vec::new();
            geom_data.extend_from_slice(&(SCREEN_WIDTH as u32).to_le_bytes());
            geom_data.extend_from_slice(&(SCREEN_HEIGHT as u32).to_le_bytes());
            root_properties.insert(net_desktop_geom, PropertyValue {
                prop_type: 6,
                format: 32,
                data: geom_data,
            });

            // _NET_DESKTOP_VIEWPORT = (0, 0)
            let net_desktop_vp = atoms_lock.intern("_NET_DESKTOP_VIEWPORT", false);
            root_properties.insert(net_desktop_vp, PropertyValue {
                prop_type: 6,
                format: 32,
                data: vec![0; 8],
            });

            // _NET_WORKAREA = (0, 0, SCREEN_WIDTH, SCREEN_HEIGHT)
            let net_workarea = atoms_lock.intern("_NET_WORKAREA", false);
            let mut workarea = Vec::new();
            workarea.extend_from_slice(&0u32.to_le_bytes());
            workarea.extend_from_slice(&0u32.to_le_bytes());
            workarea.extend_from_slice(&(SCREEN_WIDTH as u32).to_le_bytes());
            workarea.extend_from_slice(&(SCREEN_HEIGHT as u32).to_le_bytes());
            root_properties.insert(net_workarea, PropertyValue {
                prop_type: 6,
                format: 32,
                data: workarea,
            });

            // _NET_CLIENT_LIST = empty
            let net_client_list = atoms_lock.intern("_NET_CLIENT_LIST", false);
            root_properties.insert(net_client_list, PropertyValue {
                prop_type: 33, // WINDOW
                format: 32,
                data: Vec::new(),
            });

            // _NET_CLIENT_LIST_STACKING = empty
            let net_client_list_stacking = atoms_lock.intern("_NET_CLIENT_LIST_STACKING", false);
            root_properties.insert(net_client_list_stacking, PropertyValue {
                prop_type: 33,
                format: 32,
                data: Vec::new(),
            });

            // _NET_ACTIVE_WINDOW = 0 (none)
            let net_active = atoms_lock.intern("_NET_ACTIVE_WINDOW", false);
            root_properties.insert(net_active, PropertyValue {
                prop_type: 33,
                format: 32,
                data: 0u32.to_le_bytes().to_vec(),
            });

            // _NET_DESKTOP_NAMES = "Desktop\0"
            let net_desktop_names = atoms_lock.intern("_NET_DESKTOP_NAMES", false);
            root_properties.insert(net_desktop_names, PropertyValue {
                prop_type: utf8_string,
                format: 8,
                data: b"Desktop\0".to_vec(),
            });

            // _XKB_RULES_NAMES — toolkit apps (GTK, Qt) read this to configure XKB.
            // Format: five null-terminated strings: rules, model, layout, variant, options
            let xkb_rules_atom = atoms_lock.intern("_XKB_RULES_NAMES", false);
            let xkb_rules_data = b"evdev\0pc105\0us\0\0\0".to_vec();
            root_properties.insert(xkb_rules_atom, PropertyValue {
                prop_type: 31, // STRING
                format: 8,
                data: xkb_rules_data,
            });

            // _NET_WM_CM_S0 — advertise compositing manager (our server composites)
            let net_wm_cm_atom = atoms_lock.intern("_NET_WM_CM_S0", false);
            root_properties.insert(net_wm_cm_atom, PropertyValue {
                prop_type: 33, // WINDOW
                format: 32,
                data: ROOT_WINDOW.to_le_bytes().to_vec(),
            });

            // RESOURCE_MANAGER — toolkit configuration string read by Xt/GTK/Qt
            // This provides sensible defaults for font DPI and related settings.
            let resource_mgr_atom = atoms_lock.intern("RESOURCE_MANAGER", false);
            let resource_mgr_data = b"Xft.dpi:\t96\nXft.antialias:\t1\nXft.hinting:\t1\nXft.hintstyle:\thintslight\nXft.rgba:\trgb\n".to_vec();
            root_properties.insert(resource_mgr_atom, PropertyValue {
                prop_type: 31, // STRING
                format: 8,
                data: resource_mgr_data,
            });

            // WM_CHECK_WINDOW properties: _NET_SUPPORTING_WM_CHECK points to itself,
            // _NET_WM_NAME = "x11-web" (required by EWMH spec).
            let mut wm_check_properties: HashMap<u32, PropertyValue> = HashMap::new();
            let net_supporting_wm_check = atoms_lock.intern("_NET_SUPPORTING_WM_CHECK", false);
            wm_check_properties.insert(net_supporting_wm_check, PropertyValue {
                prop_type: 33, // WINDOW
                format: 32,
                data: WM_CHECK_WINDOW.to_le_bytes().to_vec(),
            });
            let net_wm_name_atom = atoms_lock.intern("_NET_WM_NAME", false);
            let utf8_string_atom = atoms_lock.intern("UTF8_STRING", false);
            wm_check_properties.insert(net_wm_name_atom, PropertyValue {
                prop_type: utf8_string_atom,
                format: 8,
                data: b"x11-web".to_vec(),
            });

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
                    class: 1,
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
                    backing_store: 0,
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
                    visual: 0,
                    class: 2, // InputOnly
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
                    backing_store: 0,
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
                },
            );

            // Insert the XSETTINGS manager window. Like the WM check window it is
            // InputOnly, unmapped, and exists solely to own the _XSETTINGS_S0
            // selection and carry the _XSETTINGS_SETTINGS property.
            let mut xsettings_properties: HashMap<u32, PropertyValue> = HashMap::new();
            {
                let mut atoms_lock = shared_atoms.lock().unwrap();
                let xsettings_settings_atom = atoms_lock.intern("_XSETTINGS_SETTINGS", false);
                xsettings_properties.insert(xsettings_settings_atom, PropertyValue {
                    prop_type: xsettings_settings_atom,
                    format: 8,
                    data: setup::build_xsettings_data(),
                });
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
                    visual: 0,
                    class: 2, // InputOnly
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
                    backing_store: 0,
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
                sels.insert(164, SelectionEntry { // 164 = _XSETTINGS_S0
                    owner: XSETTINGS_WINDOW,
                    event_tx: dummy_tx,
                    timestamp: 0,
                });
            }
        }

        // Register the server as the CLIPBOARD_MANAGER selection owner.
        // This advertises that we implement clipboard persistence per ICCCM:
        // when a CLIPBOARD owner disconnects, the server preserves the data
        // and takes over CLIPBOARD ownership.
        {
            let (mgr_tx, _mgr_rx) = mpsc::unbounded_channel();
            if let Ok(mut sels) = shared_selections.lock() {
                sels.insert(179, SelectionEntry { // 179 = CLIPBOARD_MANAGER
                    owner: types::CLIPBOARD_MANAGER_WINDOW,
                    event_tx: mgr_tx,
                    timestamp: 0,
                });
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
            xim_properties.insert(locales_atom, PropertyValue {
                prop_type: 31, // STRING
                format: 8,
                data: b"@locale=C,en,en_US,en_US.UTF-8,POSIX".to_vec(),
            });
            xim_properties.insert(transport_atom, PropertyValue {
                prop_type: 31, // STRING
                format: 8,
                data: b"@transport=X/".to_vec(),
            });

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
                    visual: 0,
                    class: 2, // InputOnly
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
                    backing_store: 0,
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
                },
            );

            // Set XIM_SERVERS property on root: list of server name atoms.
            if let Some(root) = windows.get_mut(&ROOT_WINDOW) {
                root.children_order.push(XIM_WINDOW);
                root.properties.insert(xim_servers_atom, PropertyValue {
                    prop_type: 4, // ATOM
                    format: 32,
                    data: xim_server_name_atom.to_le_bytes().to_vec(),
                });
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
                let wm = shared_wm_state.clone();
                let sa = shared_atoms.clone();
                let wr = self.window_router.clone();
                let mt = self.menu_tracker.clone();
                let er = shared_event_router.clone();
                let ss = shared_selections.clone();
                let cn = self.clipboard_notify_tx.clone();
                let sc = self.shared_clipboard.clone();
                let sp = shared_pixmaps.clone();
                let spf = shared_pixmap_fbs.clone();
                let sg = shared_gcs.clone();
                let cr = client_registry.clone();
                let eb = event_broadcaster.clone();
                let sgl = server_grab.clone();
                let src = shared_record_contexts.clone();
                let pc = self.persistent_clipboard.clone();
                let ac = self.auth_cookie;
                let ssr = self.screen_size_rx.clone();
                let sacl = self.shared_access_control.clone();
                let sst = self.shared_security_tokens.clone();
                let stream = $stream;
                tokio::spawn(async move {
                    if let Err(e) =
                        connection::handle_client(stream, client_id, update_tx, message_tx, message_rx, conn_index, sw, wm, sa, wr, mt, er, ss, cn, sc, sp, spf, sg, cr, eb, sgl, src, pc, ac, ssr, sacl, sst).await
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





/// Walk from `start` up through `parent` links collecting the chain of window IDs.
pub(crate) fn ancestor_chain(windows: &HashMap<u32, WindowState>, start: u32) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut cur = start;
    for _ in 0..128 {
        chain.push(cur);
        match windows.get(&cur).map(|w| w.parent) {
            Some(p) if p != 0 && p != cur => cur = p,
            _ => break,
        }
    }
    chain
}

/// Check if window `child` is a descendant of window `ancestor`.
pub(crate) fn is_descendant_of(windows: &HashMap<u32, WindowState>, child: u32, ancestor: u32) -> bool {
    let mut current = child;
    for _ in 0..128 {
        let parent = match windows.get(&current) {
            Some(w) => w.parent,
            None => return false,
        };
        if parent == ancestor {
            return true;
        }
        if parent == 0 {
            return false;
        }
        current = parent;
    }
    false
}

/// Compute the visibility state of a window based on its siblings' stacking order.
/// Returns: 0 = Unobscured, 1 = PartiallyObscured, 2 = FullyObscured.
pub(crate) fn compute_visibility(windows: &HashMap<u32, WindowState>, wid: u32) -> u8 {
    let (parent_id, wx, wy, ww, wh, mapped) = match windows.get(&wid) {
        Some(w) => (w.parent, w.x as i32, w.y as i32, w.width as i32, w.height as i32, w.mapped),
        None => return 2,
    };
    if !mapped || ww == 0 || wh == 0 {
        return 2; // FullyObscured — not visible
    }

    let children = match windows.get(&parent_id) {
        Some(p) => p.children_order.clone(),
        None => return 0, // No parent → root-level, assume unobscured
    };

    // Find our position in the stacking order
    let our_idx = match children.iter().position(|&c| c == wid) {
        Some(i) => i,
        None => return 0,
    };

    // Check all siblings above us (higher index = on top)
    let mut obscured_area = 0i64;
    let total_area = ww as i64 * wh as i64;
    let mut partially = false;

    for &sibling_id in &children[our_idx + 1..] {
        let sibling = match windows.get(&sibling_id) {
            Some(s) if s.mapped => s,
            _ => continue,
        };

        let sx = sibling.x as i32;
        let sy = sibling.y as i32;
        let sw = sibling.width as i32;
        let sh = sibling.height as i32;

        // Compute intersection
        let ix1 = wx.max(sx);
        let iy1 = wy.max(sy);
        let ix2 = (wx + ww).min(sx + sw);
        let iy2 = (wy + wh).min(sy + sh);

        if ix1 < ix2 && iy1 < iy2 {
            let overlap = (ix2 - ix1) as i64 * (iy2 - iy1) as i64;
            obscured_area += overlap;
            partially = true;
        }
    }

    if !partially {
        0 // Unobscured
    } else if obscured_area >= total_area {
        2 // FullyObscured (conservative: not accounting for overlap between siblings)
    } else {
        1 // PartiallyObscured
    }
}

fn handle_request(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 4 {
        return Vec::new();
    }
    let major_opcode = data[0];
    let _minor = data[1];
    let seq = state.sequence;
    // Extension major opcodes (assigned by QueryExtension)
    const EXT_SHAPE: u8 = 128;
    const EXT_SHM: u8 = 130;
    const EXT_XINPUT: u8 = 131;
    const EXT_BIG_REQUESTS: u8 = 133;
    const EXT_SYNC: u8 = 134;
    const EXT_GE: u8 = 135;
    const EXT_XKB: u8 = 136;
    const EXT_XFIXES: u8 = 138;
    const EXT_RENDER: u8 = 139;
    const EXT_RANDR: u8 = 140;
    const EXT_XC_MISC: u8 = 141;
    const EXT_COMPOSITE: u8 = 142;
    const EXT_DAMAGE: u8 = 143;
    const EXT_PRESENT: u8 = 148;
    const EXT_DRI3: u8 = 149;
    const EXT_XTEST: u8 = 150;
    const EXT_DPMS: u8 = 151;
    const EXT_SCREEN_SAVER: u8 = 152;
    const EXT_VIDMODE: u8 = 153;
    const EXT_RECORD: u8 = 154;
    const EXT_SECURITY: u8 = 155;
    const EXT_XVIDEO: u8 = 156;
    const EXT_DBE: u8 = 157;
    const EXT_XINERAMA: u8 = 158;
    const EXT_GLX: u8 = 159;
    const EXT_XRESOURCE: u8 = 160;

    match major_opcode {
        // Core protocol requests (opcodes 1-127)
        1..=127 => handlers::handle_core_request(state, data),

        EXT_BIG_REQUESTS => {
            // BigReqEnable: mark BIG-REQUESTS as enabled and return max request length.
            state.big_requests_enabled = true;
            let bo = state.msb_first;
            let mut reply = [0u8; 32];
            reply[0] = 1;
            write_u16_bo(&mut reply, 2, seq, bo);
            write_u32_bo(&mut reply, 8, 4194304u32, bo); // 16MB / 4 = 4194304 words
            reply.to_vec()
        }

        // Extension protocol requests
        EXT_SHAPE => handlers::extensions::handle_shape_request(state, data, seq),
        EXT_SHM => handlers::extensions::handle_shm_request(state, data, seq),
        EXT_XINPUT => {
            let mut reply = crate::xinput2::handle_request(
                data,
                seq,
                &mut state.xi.valuators,
                &mut state.xi.selections,
                &mut state.xi.pending,
                &mut state.xi.client_pointer,
                &mut state.xi.device_properties,
                &mut state.focus_window,
                &mut state.xi.active_grabs,
                &mut state.xi.passive_grabs,
                &mut state.xi.pointer_frozen,
                &mut state.xi.keyboard_frozen,
                &mut state.xi.frozen_pointer_events,
                &mut state.xi.frozen_keyboard_events,
                SCREEN_WIDTH,
                SCREEN_HEIGHT,
                state.root_window,
                state.msb_first,
            );
            if data.len() >= 2 && data[1] == x11rb_protocol::protocol::xinput::XI_QUERY_POINTER_REQUEST
                && reply.len() >= 12
            {
                crate::xinput2::patch_query_pointer_root(&mut reply, state.root_window, state.msb_first);
            }
            reply
        }
        EXT_SYNC => handlers::extensions::handle_sync_request(state, data, seq),
        EXT_GE => handlers::extensions::handle_ge_request(state, data, seq),
        EXT_XKB => handlers::extensions::handle_xkb_request(state, data, seq),
        EXT_XFIXES => handlers::extensions::handle_xfixes_request(state, data, seq),
        EXT_RENDER => handlers::render::handle_render_request(state, data, seq),
        EXT_RANDR => handlers::extensions::handle_randr_request(state, data, seq),
        EXT_XC_MISC => handlers::extensions::handle_xc_misc_request(state, data, seq),
        EXT_COMPOSITE => handlers::extensions::handle_x_composite_request(state, data, seq),
        EXT_DAMAGE => handlers::extensions::handle_damage_request(state, data, seq),
        EXT_PRESENT => handlers::extensions::handle_present_request(state, data, seq),
        EXT_DRI3 => handlers::extensions::handle_dri3_request(state, data, seq),
        EXT_XTEST => handlers::extensions::handle_xtest_request(state, data, seq),
        EXT_DPMS => handlers::extensions::handle_dpms_request(state, data, seq),
        EXT_SCREEN_SAVER => handlers::extensions::handle_screen_saver_request(state, data, seq),
        EXT_VIDMODE => handlers::extensions::handle_vidmode_request(state, data, seq),
        EXT_RECORD => handlers::record::handle_record_request(state, data, seq),
        EXT_SECURITY => handlers::extensions::handle_security_request(state, data, seq),
        EXT_XVIDEO => handlers::extensions::handle_xvideo_request(state, data, seq),
        EXT_DBE => handlers::extensions::handle_dbe_request(state, data, seq),
        EXT_XINERAMA => handlers::extensions::handle_xinerama_request(state, data, seq),
        EXT_GLX => handlers::extensions::handle_glx_request(state, data, seq),
        EXT_XRESOURCE => handlers::extensions::handle_xresource_request(state, data, seq),
        _ => {
            warn!("Unhandled X11 request opcode: {major_opcode} minor: {_minor}");
            // Return BadRequest error per spec for unrecognized opcodes
            self::core::build_error_bo(
                BAD_REQUEST, seq, major_opcode as u32,
                major_opcode, _minor as u16, state.msb_first,
            )
        }
    }
}
