//! X11 server implementation.
//!
//! This module implements a minimal but spec-compliant X11 server that
//! translates X11 protocol requests into DisplayUpdate messages for the frontend.

pub(crate) mod atoms;
pub(crate) mod client;
#[allow(dead_code)]
pub(crate) mod core;
#[allow(dead_code)]
pub(crate) mod grab;
pub(crate) mod handlers;
pub(crate) mod types;

// Re-exports used by main.rs and other crates
pub use types::{TaggedDisplayUpdate, WindowRouter};
// Re-exports used by render.rs and other sibling modules
pub(crate) use client::ClientState;
pub(crate) use core::build_error;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::sync::mpsc;
use tokio::time::Duration;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use x11rb_protocol::protocol::xproto::{
    BackingStore, Depth, EventMask, Format, ImageOrder, Screen, Setup, SetupRequest,
    Visualtype, VisualClass,
};
use x11rb_protocol::x11_utils::{Serialize, TryParse};
use x11_web_protocol::{DisplayUpdate, InputEvent};

use crate::fonts::FontManager;
use crate::framebuffer::Framebuffer;

use self::atoms::AtomManager;
use self::core::*;
use self::grab::GrabState;
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
}

impl X11Server {
    pub fn new(
        display_number: u32,
        update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
        client_connected_tx: mpsc::UnboundedSender<(String, u32)>,
        window_router: WindowRouter,
        menu_tracker: crate::menus::MenuTracker,
    ) -> Self {
        let socket_path = PathBuf::from(format!("/tmp/.X11-unix/X{display_number}"));
        Self {
            display_number,
            socket_path,
            update_tx,
            client_connected_tx,
            window_router,
            menu_tracker,
        }
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

        // Pre-populate with root window
        {
            let mut windows = shared_windows.lock().unwrap();
            let mut root_properties: HashMap<u32, PropertyValue> = HashMap::new();

            let mut atoms_lock = shared_atoms.lock().unwrap();
            let atom_shows_menubar =
                atoms_lock.intern("_GTK_SHELL_SHOWS_MENUBAR", false);
            let atom_shows_app_menu =
                atoms_lock.intern("_GTK_SHELL_SHOWS_APP_MENU", false);
            drop(atoms_lock);

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
                    background_pixel: 0x00000000,
                    override_redirect: false,
                    redirected: false,
                    framebuffer: Framebuffer::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32),
                    properties: root_properties,
                    owner_client_id: String::new(),
                    cursor: None,
                },
            );
        }

        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let conn_index = CONNECTION_COUNTER.fetch_add(1, Ordering::Relaxed);
                    let client_id = Uuid::new_v4().to_string();
                    let peer_pid = stream.peer_cred().ok().and_then(|c| c.pid()).unwrap_or(0) as u32;
                    let update_tx = self.update_tx.clone();
                    let (message_tx, message_rx) = mpsc::unbounded_channel();
                    let _ = self.client_connected_tx.send((client_id.clone(), peer_pid));
                    let cid = client_id.clone();
                    let sw = shared_windows.clone();
                    let wm = shared_wm_state.clone();
                    let sa = shared_atoms.clone();
                    let wr = self.window_router.clone();
                    let mt = self.menu_tracker.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            handle_client(stream, client_id, update_tx, message_tx, message_rx, conn_index, sw, wm, sa, wr, mt).await
                        {
                            debug!("X11 client {cid} disconnected: {e}");
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept X11 connection: {e}");
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

fn build_setup(conn_index: u32) -> Setup {
    let visual = Visualtype {
        visual_id: ROOT_VISUAL,
        class: VisualClass::TRUE_COLOR,
        bits_per_rgb_value: 8,
        colormap_entries: 256,
        red_mask: 0x00FF0000,
        green_mask: 0x0000FF00,
        blue_mask: 0x000000FF,
    };

    let depth24 = Depth {
        depth: 24,
        visuals: vec![visual],
    };

    let visual_argb = Visualtype {
        visual_id: 0x40,
        class: VisualClass::TRUE_COLOR,
        bits_per_rgb_value: 8,
        colormap_entries: 256,
        red_mask: 0x00FF0000,
        green_mask: 0x0000FF00,
        blue_mask: 0x000000FF,
    };

    let depth32 = Depth {
        depth: 32,
        visuals: vec![visual_argb],
    };

    let screen = Screen {
        root: ROOT_WINDOW,
        default_colormap: ROOT_COLORMAP,
        white_pixel: 0x00FFFFFF,
        black_pixel: 0x00000000,
        current_input_masks: EventMask::from(0u32),
        width_in_pixels: SCREEN_WIDTH,
        height_in_pixels: SCREEN_HEIGHT,
        width_in_millimeters: 270,
        height_in_millimeters: 203,
        min_installed_maps: 1,
        max_installed_maps: 1,
        root_visual: ROOT_VISUAL,
        backing_stores: BackingStore::NOT_USEFUL,
        save_unders: false,
        root_depth: 24,
        allowed_depths: vec![depth24, depth32],
    };

    let format24 = Format { depth: 24, bits_per_pixel: 32, scanline_pad: 32 };
    let format32 = Format { depth: 32, bits_per_pixel: 32, scanline_pad: 32 };
    let format1 = Format { depth: 1, bits_per_pixel: 1, scanline_pad: 32 };

    let mut setup = Setup {
        status: 1,
        protocol_major_version: 11,
        protocol_minor_version: 0,
        length: 0,
        release_number: 0,
        resource_id_base: ((conn_index + 1) as u32) << 22,
        resource_id_mask: 0x003FFFFF,
        motion_buffer_size: 256,
        maximum_request_length: 65535,
        image_byte_order: ImageOrder::LSB_FIRST,
        bitmap_format_bit_order: ImageOrder::LSB_FIRST,
        bitmap_format_scanline_unit: 32,
        bitmap_format_scanline_pad: 32,
        min_keycode: 8,
        max_keycode: 255,
        vendor: b"x11-web".to_vec(),
        pixmap_formats: vec![format1, format24, format32],
        roots: vec![screen],
    };

    let mut bytes = Vec::new();
    setup.serialize_into(&mut bytes);
    setup.length = ((bytes.len() - 8) / 4) as u16;

    setup
}

async fn handle_client(
    mut stream: tokio::net::UnixStream,
    client_id: String,
    update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
    message_tx: mpsc::UnboundedSender<(u32, WindowMessage)>,
    mut message_rx: mpsc::UnboundedReceiver<(u32, WindowMessage)>,
    conn_index: u32,
    shared_windows: SharedWindows,
    shared_wm_state: SharedWmState,
    shared_atoms: Arc<Mutex<AtomManager>>,
    window_router: WindowRouter,
    menu_tracker: crate::menus::MenuTracker,
) -> io::Result<()> {
    // Phase 1: Read client setup request
    let mut header_buf = [0u8; 12];
    stream.read_exact(&mut header_buf).await?;

    let byte_order = header_buf[0];
    if byte_order != 0x6c && byte_order != 0x42 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid byte order: 0x{:02x}", byte_order),
        ));
    }

    let (auth_name_len, auth_data_len) = if byte_order == 0x6c {
        (
            u16::from_le_bytes([header_buf[6], header_buf[7]]),
            u16::from_le_bytes([header_buf[8], header_buf[9]]),
        )
    } else {
        (
            u16::from_be_bytes([header_buf[6], header_buf[7]]),
            u16::from_be_bytes([header_buf[8], header_buf[9]]),
        )
    };

    fn pad4(n: u16) -> usize {
        let n = n as usize;
        (n + 3) & !3
    }
    let total_len = 12 + pad4(auth_name_len) + pad4(auth_data_len);
    let mut setup_buf = vec![0u8; total_len];
    setup_buf[..12].copy_from_slice(&header_buf);
    if total_len > 12 {
        stream.read_exact(&mut setup_buf[12..]).await?;
    }

    let _setup_request = SetupRequest::try_parse(&setup_buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Bad setup: {e:?}")))?;

    // Phase 2: Send setup reply
    let setup = build_setup(conn_index);
    let mut reply_bytes = Vec::new();
    setup.serialize_into(&mut reply_bytes);
    stream.write_all(&reply_bytes).await?;

    info!("X11 client connected: {client_id}");

    // Phase 3: Handle requests
    let local_windows = shared_windows.lock().unwrap().clone();
    let (wm_events_tx, mut wm_events_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let mut state = ClientState {
        client_id: client_id.clone(),
        sequence: 0,
        windows: local_windows,
        shared_windows,
        pixmaps: HashMap::new(),
        gcs: HashMap::new(),
        atoms: shared_atoms,
        update_tx,
        root_window: ROOT_WINDOW,
        pointer_x: 0,
        pointer_y: 0,
        focus_window: ROOT_WINDOW,
        font_manager: FontManager::new(),
        render: crate::render::RenderState::new(),
        selections: HashMap::new(),
        shm_segments: HashMap::new(),
        wm_state: shared_wm_state.clone(),
        wm_events_tx,
        damage_regions: HashMap::new(),
        present_subscriptions: HashMap::new(),
        pending_events: Vec::new(),
        window_router,
        message_tx,
        x11_to_uuid: HashMap::new(),
        cursors: HashMap::new(),
        xi: crate::xinput2::XiState::default(),
        menu_tracker,
        gtk_menu_paths: HashMap::new(),
        grabs: GrabState::default(),
        close_down_mode: 0,
        pressed_keys: [0u8; 32],
        keyboard_control: Default::default(),
        pointer_control: Default::default(),
        screen_saver: Default::default(),
    };

    let mut buf = vec![0u8; 256 * 1024];
    let mut pending = Vec::new();
    let mut frame_interval = tokio::time::interval(Duration::from_millis(100));

    let _wm_guard = WmCleanupGuard {
        wm_state: shared_wm_state,
        client_id: client_id.clone(),
    };

    loop {
        tokio::select! {
            result = stream.read(&mut buf) => {
                let n = result?;
                if n == 0 {
                    let uuids: Vec<String> = state.x11_to_uuid.values().cloned().collect();
                    state.window_router.unregister_all(&uuids);
                    return Ok(());
                }

                pending.extend_from_slice(&buf[..n]);
                state.sync_windows();

                while pending.len() >= 4 {
                    let req_len_units = u16::from_le_bytes([pending[2], pending[3]]) as usize;
                    let req_len_bytes = req_len_units * 4;

                    if req_len_bytes == 0 {
                        if pending.len() < 8 {
                            break;
                        }
                        let big_len =
                            u32::from_le_bytes([pending[4], pending[5], pending[6], pending[7]]) as usize;
                        let big_bytes = big_len * 4;
                        if pending.len() < big_bytes {
                            break;
                        }
                        state.sequence = state.sequence.wrapping_add(1);
                        pending.drain(..big_bytes);
                        continue;
                    }

                    if pending.len() < req_len_bytes {
                        break;
                    }

                    let request_data: Vec<u8> = pending.drain(..req_len_bytes).collect();
                    state.sequence = state.sequence.wrapping_add(1);

                    let response = handle_request(&mut state, &request_data);
                    if !response.is_empty() {
                        stream.write_all(&response).await?;
                    }
                }

                state.sync_windows();
                state.flush_dirty_windows();

                for event in state.pending_events.drain(..) {
                    stream.write_all(&event).await?;
                }
            }
            _ = frame_interval.tick() => {
                state.sync_windows();
                state.flush_dirty_windows();

                if state.xi.pending.raw_motion {
                    state.xi.pending.raw_motion = false;
                    let ev = crate::xinput2::build_raw_motion_event(state.sequence);
                    state.pending_events.push(ev);
                }

                for event in state.pending_events.drain(..) {
                    stream.write_all(&event).await?;
                }

                state.sync_windows();
            }
            Some((x11_wid, msg)) = message_rx.recv() => {
                match msg {
                    WindowMessage::Input(input) => {
                        if let x11_web_protocol::InputEvent::MenuActivate { action } = &input {
                            if let Some(uuid) = state.top_level_uuid_for(x11_wid) {
                                state.menu_tracker.activate(&uuid, action.clone());
                            }
                            continue;
                        }
                        state.set_focus_window(x11_wid);
                        if matches!(
                            input,
                            x11_web_protocol::InputEvent::ButtonPress { .. }
                        ) {
                            let uuid = state.top_level_uuid_for(x11_wid);
                            state.broadcast_focus(uuid);
                        }
                        match &input {
                            x11_web_protocol::InputEvent::MotionNotify { x, y, .. }
                            | x11_web_protocol::InputEvent::ButtonPress { x, y, .. }
                            | x11_web_protocol::InputEvent::ButtonRelease { x, y, .. } => {
                                state.xi.valuators.x = *x as i32;
                                state.xi.valuators.y = *y as i32;
                            }
                            _ => {}
                        }

                        // Track pressed keys for QueryKeymap
                        match &input {
                            x11_web_protocol::InputEvent::KeyPress { keycode, .. } => {
                                let kc = *keycode as usize;
                                if kc < 256 {
                                    state.pressed_keys[kc / 8] |= 1 << (kc % 8);
                                }
                            }
                            x11_web_protocol::InputEvent::KeyRelease { keycode, .. } => {
                                let kc = *keycode as usize;
                                if kc < 256 {
                                    state.pressed_keys[kc / 8] &= !(1 << (kc % 8));
                                }
                            }
                            _ => {}
                        }

                        let event_bytes = build_x11_input_event(&mut state, &input, x11_wid);
                        if !event_bytes.is_empty() {
                            stream.write_all(&event_bytes).await?;
                        }
                        let chain = ancestor_chain(&state.windows, x11_wid);
                        let xi_events = crate::xinput2::build_xi_events_for(
                            &mut state.xi.valuators,
                            &state.xi.selections,
                            &chain,
                            state.sequence,
                            state.root_window,
                            &input,
                        );
                        for ev in xi_events {
                            stream.write_all(&ev).await?;
                        }
                    }
                    WindowMessage::Resize(width, height) => {
                        if let Some(uuid) = state.x11_to_uuid.get(&x11_wid).cloned() {
                            let events = resize_window(&mut state, &uuid, width, height);
                            if !events.is_empty() {
                                stream.write_all(&events).await?;
                            }
                        }
                    }
                }
            }
            Some(event_data) = wm_events_rx.recv() => {
                stream.write_all(&event_data).await?;
            }
        }
    }
}

/// Resize a top-level window in response to a frontend canvas size change.
fn resize_window(state: &mut ClientState, window_uuid: &str, width: u16, height: u16) -> Vec<u8> {
    let mut events = Vec::new();
    let seq = state.sequence;

    let window_id = match state.x11_to_uuid.iter().find(|(_, uuid)| uuid.as_str() == window_uuid) {
        Some((&wid, _)) => wid,
        None => return events,
    };

    if let Some(win) = state.windows.get_mut(&window_id) {
        win.width = width;
        win.height = height;
        win.framebuffer = Framebuffer::new(width as u32, height as u32);

        let mut event = [0u8; 32];
        event[0] = CONFIGURE_NOTIFY_EVENT;
        event[2..4].copy_from_slice(&seq.to_le_bytes());
        event[4..8].copy_from_slice(&window_id.to_le_bytes());
        event[8..12].copy_from_slice(&window_id.to_le_bytes());
        event[16..18].copy_from_slice(&win.x.to_le_bytes());
        event[18..20].copy_from_slice(&win.y.to_le_bytes());
        event[20..22].copy_from_slice(&width.to_le_bytes());
        event[22..24].copy_from_slice(&height.to_le_bytes());
        event[24..26].copy_from_slice(&win.border_width.to_le_bytes());
        events.extend_from_slice(&event);
    }

    let exposed: Vec<(u32, u16, u16)> = std::iter::once(window_id)
        .chain(
            state
                .windows
                .values()
                .filter(|w| {
                    w.mapped && w.id != window_id && is_descendant_of(&state.windows, w.id, window_id)
                })
                .map(|w| w.id),
        )
        .filter_map(|wid| state.windows.get(&wid).map(|w| (wid, w.width, w.height)))
        .collect();
    for (wid, w, h) in exposed {
        let mut expose = [0u8; 32];
        expose[0] = EXPOSE_EVENT;
        expose[2..4].copy_from_slice(&seq.to_le_bytes());
        expose[4..8].copy_from_slice(&wid.to_le_bytes());
        expose[12..14].copy_from_slice(&w.to_le_bytes());
        expose[14..16].copy_from_slice(&h.to_le_bytes());
        events.extend_from_slice(&expose);
    }

    if let Some(win) = state.windows.get(&window_id) {
        let owner = if win.owner_client_id.is_empty() {
            state.client_id.clone()
        } else {
            win.owner_client_id.clone()
        };
        let _ = state.update_tx.send((
            owner,
            DisplayUpdate::WindowConfigured {
                window_id: window_uuid.to_string(),
                x: win.x,
                y: win.y,
                width: win.width,
                height: win.height,
            },
        ));
    }

    events
}

/// Walk from `start` up through `parent` links collecting the chain of window IDs.
pub(crate) fn ancestor_chain(windows: &HashMap<u32, WindowState>, start: u32) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut cur = start;
    for _ in 0..32 {
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
    for _ in 0..20 {
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

/// Walk the window tree to find the deepest mapped descendant under the point
/// that has selected for the requested event mask.
fn find_event_subwindow(
    windows: &HashMap<u32, WindowState>,
    parent: u32,
    rel_x: i16,
    rel_y: i16,
    required_mask: u32,
) -> (u32, i16, i16) {
    fn descend(
        windows: &HashMap<u32, WindowState>,
        parent: u32,
        rel_x: i16,
        rel_y: i16,
        required_mask: u32,
        best: &mut Option<(u32, i16, i16)>,
    ) {
        if let Some(w) = windows.get(&parent) {
            if w.event_mask & required_mask != 0 {
                *best = Some((parent, rel_x, rel_y));
            }
        }
        let children: Vec<&WindowState> = windows
            .values()
            .filter(|w| w.parent == parent && w.mapped)
            .collect();
        for child in children {
            let cx = child.x;
            let cy = child.y;
            let cw = child.width as i16;
            let ch = child.height as i16;
            if rel_x >= cx && rel_x < cx + cw && rel_y >= cy && rel_y < cy + ch {
                descend(windows, child.id, rel_x - cx, rel_y - cy, required_mask, best);
            }
        }
    }

    let mut best: Option<(u32, i16, i16)> = None;
    descend(windows, parent, rel_x, rel_y, required_mask, &mut best);
    best.unwrap_or((parent, rel_x, rel_y))
}

/// Convert a frontend InputEvent into X11 wire-format event bytes.
fn build_x11_input_event(state: &mut ClientState, input: &InputEvent, top_level: u32) -> Vec<u8> {
    match input {
        InputEvent::MotionNotify { x, y, .. }
        | InputEvent::ButtonPress { x, y, .. }
        | InputEvent::ButtonRelease { x, y, .. } => {
            state.pointer_x = *x;
            state.pointer_y = *y;
        }
        _ => {}
    }

    let seq = state.sequence;
    let mut event = [0u8; 32];
    let timestamp: u32 = 0;

    let (event_window, event_x, event_y) = match input {
        InputEvent::MotionNotify { x, y, .. } => {
            find_event_subwindow(&state.windows, top_level, *x, *y, POINTER_MOTION_MASK)
        }
        InputEvent::ButtonPress { x, y, .. } => {
            find_event_subwindow(&state.windows, top_level, *x, *y, BUTTON_PRESS_MASK)
        }
        InputEvent::ButtonRelease { x, y, .. } => {
            find_event_subwindow(&state.windows, top_level, *x, *y, BUTTON_RELEASE_MASK)
        }
        _ => (top_level, 0, 0),
    };

    match input {
        InputEvent::MotionNotify { x, y, state: mask } => {
            event[0] = MOTION_NOTIFY_EVENT;
            event[1] = 0;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[4..8].copy_from_slice(&timestamp.to_le_bytes());
            event[8..12].copy_from_slice(&state.root_window.to_le_bytes());
            event[12..16].copy_from_slice(&event_window.to_le_bytes());
            event[16..20].copy_from_slice(&0u32.to_le_bytes());
            event[20..22].copy_from_slice(&x.to_le_bytes());
            event[22..24].copy_from_slice(&y.to_le_bytes());
            event[24..26].copy_from_slice(&event_x.to_le_bytes());
            event[26..28].copy_from_slice(&event_y.to_le_bytes());
            event[28..30].copy_from_slice(&mask.to_le_bytes());
            event[30] = 1;
        }
        InputEvent::ButtonPress { button, x, y, state: mask } => {
            event[0] = BUTTON_PRESS_EVENT;
            event[1] = *button;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[4..8].copy_from_slice(&timestamp.to_le_bytes());
            event[8..12].copy_from_slice(&state.root_window.to_le_bytes());
            event[12..16].copy_from_slice(&event_window.to_le_bytes());
            event[16..20].copy_from_slice(&0u32.to_le_bytes());
            event[20..22].copy_from_slice(&x.to_le_bytes());
            event[22..24].copy_from_slice(&y.to_le_bytes());
            event[24..26].copy_from_slice(&event_x.to_le_bytes());
            event[26..28].copy_from_slice(&event_y.to_le_bytes());
            event[28..30].copy_from_slice(&mask.to_le_bytes());
            event[30] = 1;
        }
        InputEvent::ButtonRelease { button, x, y, state: mask } => {
            event[0] = BUTTON_RELEASE_EVENT;
            event[1] = *button;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[4..8].copy_from_slice(&timestamp.to_le_bytes());
            event[8..12].copy_from_slice(&state.root_window.to_le_bytes());
            event[12..16].copy_from_slice(&event_window.to_le_bytes());
            event[16..20].copy_from_slice(&0u32.to_le_bytes());
            event[20..22].copy_from_slice(&x.to_le_bytes());
            event[22..24].copy_from_slice(&y.to_le_bytes());
            event[24..26].copy_from_slice(&event_x.to_le_bytes());
            event[26..28].copy_from_slice(&event_y.to_le_bytes());
            event[28..30].copy_from_slice(&mask.to_le_bytes());
            event[30] = 1;
        }
        InputEvent::KeyPress { keycode, state: mask } => {
            event[0] = KEY_PRESS_EVENT;
            event[1] = *keycode as u8;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[4..8].copy_from_slice(&timestamp.to_le_bytes());
            event[8..12].copy_from_slice(&state.root_window.to_le_bytes());
            event[12..16].copy_from_slice(&top_level.to_le_bytes());
            event[16..20].copy_from_slice(&top_level.to_le_bytes());
            event[28..30].copy_from_slice(&mask.to_le_bytes());
            event[30] = 1;
        }
        InputEvent::KeyRelease { keycode, state: mask } => {
            event[0] = KEY_RELEASE_EVENT;
            event[1] = *keycode as u8;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[4..8].copy_from_slice(&timestamp.to_le_bytes());
            event[8..12].copy_from_slice(&state.root_window.to_le_bytes());
            event[12..16].copy_from_slice(&top_level.to_le_bytes());
            event[16..20].copy_from_slice(&top_level.to_le_bytes());
            event[28..30].copy_from_slice(&mask.to_le_bytes());
            event[30] = 1;
        }
        InputEvent::MenuActivate { .. } => return Vec::new(),
    }

    event.to_vec()
}

fn handle_request(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let major_opcode = data[0];
    let _minor = data[1];
    let seq = state.sequence;
    match major_opcode {
        // Core protocol requests - delegated to handlers module
        1..=127 => handlers::handle_core_request(state, data),
        // BIG-REQUESTS
        133 => {
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&(4194303u32).to_le_bytes());
            reply.to_vec()
        }
        // Extensions
        128 => handlers::extensions::handle_shape_request(state, data, seq),
        130 => handlers::extensions::handle_shm_request(state, data, seq),
        131 => {
            let mut reply = crate::xinput2::handle_request(
                data,
                seq,
                &state.xi.valuators,
                &mut state.xi.selections,
                &mut state.xi.pending,
                SCREEN_WIDTH,
                SCREEN_HEIGHT,
                state.root_window,
            );
            if data.len() >= 2 && data[1] == x11rb_protocol::protocol::xinput::XI_QUERY_POINTER_REQUEST
                && reply.len() >= 12
            {
                crate::xinput2::patch_query_pointer_root(&mut reply, state.root_window);
            }
            reply
        }
        134 => handlers::extensions::handle_sync_request(state, data, seq),
        135 => handlers::extensions::handle_ge_request(data, seq),
        136 => handlers::extensions::handle_xkb_request(data, seq),
        138 => handlers::extensions::handle_xfixes_request(state, data, seq),
        139 => crate::render::handle_render_request(state, data, seq),
        140 => handlers::extensions::handle_randr_request(state, data, seq),
        141 => handlers::extensions::handle_xc_misc_request(data, seq),
        142 => handlers::extensions::handle_x_composite_request(state, data, seq),
        143 => handlers::extensions::handle_damage_request(state, data, seq),
        148 => handlers::extensions::handle_present_request(state, data, seq),
        _ => {
            warn!("Unhandled X11 request opcode: {major_opcode} minor: {_minor}");
            Vec::new()
        }
    }
}
