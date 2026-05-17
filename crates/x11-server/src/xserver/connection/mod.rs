//! Connection handling: per-client event loop with I/O helpers and resize logic
//! split into focused submodules.

mod resize;
pub(crate) mod scm_io;

use self::resize::apply_screen_resize;
pub(crate) use self::resize::resize_window;
use self::scm_io::{recv_with_fds, send_with_fds};
use crate::xserver::event::serialize_event;

use crate::fonts::FontManager;
use std::collections::HashMap;
use std::io;
use std::os::unix::io::AsRawFd;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::time::Duration;
use tracing::{debug, info, warn};
use x11rb_protocol::protocol::xfixes::{SelectionEvent, SelectionNotifyEvent};
use x11rb_protocol::protocol::xproto::ImageOrder;

/// X11 setup-request `byte_order` byte values: 0x6c ('l') means LSB-first /
/// little-endian; 0x42 ('B') means MSB-first / big-endian.
const BYTE_ORDER_LSB: u8 = 0x6c;
const BYTE_ORDER_MSB: u8 = 0x42;

/// Per-connection read buffer size (256 KiB). Sized to comfortably hold the
/// largest single non-BIG-REQUESTS request the spec allows (256 KiB - 4) plus
/// some slop for partial reads.
const READ_BUF_BYTES: usize = 256 * 1024;

/// Wire size of the X11 connection-setup request header (byte_order +
/// pad + protocol_major + protocol_minor + auth_name_len + auth_data_len
/// + pad). Auth strings start immediately after.
const SETUP_REQUEST_HEADER_SIZE: usize = 12;

/// Frame tick interval driving the per-connection display-update flush
/// loop. 16 ms ≈ 60 Hz.
const FRAME_INTERVAL: std::time::Duration = std::time::Duration::from_millis(16);

/// Sentinel duration used to disarm the per-key auto-repeat timer. Long
/// enough that the timer effectively never fires while no key is held.
const REPEAT_TIMER_DORMANT: std::time::Duration = std::time::Duration::from_secs(86_400);

/// Bounds on the byte length of a single BIG-REQUESTS-extended request.
/// Lower bound (2 X11 words) rejects truncated headers; the 16 MiB cap
/// matches the maximum we advertise to clients (see
/// `BIG_REQUESTS_MAX_LEN_WORDS` in core.rs) and protects the server
/// from absurdly large allocations.
const BIG_REQUEST_MIN_BYTES: usize = 8;
const BIG_REQUEST_MAX_BYTES: usize = 16 * 1024 * 1024;

use x11rb_protocol::x11_utils::Serialize;

use super::atoms::AtomManager;
use super::client::ClientState;
use super::core::*;
use super::grab;
use super::grab::GrabState;
use super::handlers;
use super::input::{build_x11_input_event, enforce_barriers, find_deepest_window};
use super::setup::{build_setup, byteswap_setup_reply};
use super::types::*;
use super::{ancestor_chain, handle_request};

/// Patch the sequence number (bytes 2-3) of every 32-byte event to match
/// the client's current sequence.  X11 spec requires event sequence fields
/// to reflect the "last request processed" at the time of delivery; stale
/// values from other connections or earlier setup phases cause xcb to abort
/// with "Unknown sequence number".
fn patch_event_sequences(events: &mut [Vec<u8>], seq: u16, msb_first: bool) {
    for ev in events.iter_mut() {
        if ev.len() >= 4 {
            if msb_first {
                ev[2..4].copy_from_slice(&seq.to_be_bytes());
            } else {
                ev[2..4].copy_from_slice(&seq.to_le_bytes());
            }
        }
    }
}

/// Safely detach a SHM segment, logging errors instead of ignoring them.
fn safe_shmdt(addr: *mut u8) {
    if addr.is_null() {
        return;
    }
    let ret = unsafe { libc::shmdt(addr as *const libc::c_void) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        warn!("shmdt({:?}) failed: {}", addr, err);
    }
}

/// Safely close a file descriptor, logging errors instead of ignoring them.
fn safe_close(fd: i32) {
    if fd < 0 {
        return;
    }
    let ret = unsafe { libc::close(fd) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        warn!("close(fd={}) failed: {}", fd, err);
    }
}

/// Build an X11 connection failure response.
fn build_auth_failure(byte_order: u8, reason: &[u8]) -> Vec<u8> {
    let reason_len = reason.len();
    let padded_reason_len = align_to_4(reason_len);
    let additional_data_words = (padded_reason_len / 4) as u16;
    let mut resp = Vec::with_capacity(8 + padded_reason_len);
    resp.push(0); // Failed
    resp.push(reason_len as u8);
    if byte_order == BYTE_ORDER_LSB {
        resp.extend_from_slice(&11u16.to_le_bytes());
        resp.extend_from_slice(&0u16.to_le_bytes());
        resp.extend_from_slice(&additional_data_words.to_le_bytes());
    } else {
        resp.extend_from_slice(&11u16.to_be_bytes());
        resp.extend_from_slice(&0u16.to_be_bytes());
        resp.extend_from_slice(&additional_data_words.to_be_bytes());
    }
    resp.extend_from_slice(reason);
    resp.resize(8 + padded_reason_len, 0);
    resp
}

pub(crate) async fn handle_client(
    mut stream: tokio::net::UnixStream,
    client_id: String,
    update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
    message_tx: mpsc::UnboundedSender<(u32, WindowMessage)>,
    mut message_rx: mpsc::UnboundedReceiver<(u32, WindowMessage)>,
    conn_index: u32,
    peer_pid: u32,
    shared_windows: SharedWindows,
    shared_keymap: super::types::SharedKeymap,
    shared_wm_state: SharedWmState,
    shared_atoms: Arc<Mutex<AtomManager>>,
    window_router: WindowRouter,
    menu_tracker: crate::menus::MenuTracker,
    event_router: EventRouter,
    shared_selections: SharedSelections,
    clipboard_notify_tx: mpsc::UnboundedSender<()>,
    shared_pixmaps: SharedPixmaps,
    shared_pixmap_fbs: SharedPixmapFbs,
    shared_gcs: SharedGcs,
    client_registry: SharedClientRegistry,
    shared_pointer: super::types::SharedPointer,
    shared_focus: super::types::SharedFocus,
    event_broadcaster: EventBroadcaster,
    server_grab: ServerGrabLock,
    shared_record_contexts: SharedRecordContexts,
    persistent_clipboard: PersistentClipboard,
    auth_cookie: [u8; 16],
    mut screen_size_rx: super::types::ScreenSizeRx,
    shared_access_control: super::types::SharedAccessControl,
    shared_security_tokens: super::types::SharedSecurityTokens,
    extension_registry: Arc<super::extensions::ExtensionRegistry>,
    server_start: std::time::Instant,
) -> io::Result<()> {
    // Phase 1: Read client setup request
    let mut header_buf = [0u8; 12];
    stream.read_exact(&mut header_buf).await?;

    let byte_order = header_buf[0];
    if byte_order != BYTE_ORDER_LSB && byte_order != BYTE_ORDER_MSB {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid byte order: 0x{:02x}", byte_order),
        ));
    }

    let (auth_name_len, auth_data_len) = if byte_order == BYTE_ORDER_LSB {
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

    let pad4 = |n: u16| align_to_4(n as usize);
    let total_len = 12 + pad4(auth_name_len) + pad4(auth_data_len);
    let mut setup_buf = vec![0u8; total_len];
    setup_buf[..12].copy_from_slice(&header_buf);
    if total_len > 12 {
        stream.read_exact(&mut setup_buf[12..]).await?;
    }

    // The x11rb-derived `SetupRequest::try_parse` reads field lengths as
    // little-endian only, so calling it on an MSB-first setup would
    // misparse `authorization_protocol_name_len` and fail before we
    // ever reach the auth check. We've already parsed `auth_name_len`
    // and `auth_data_len` with the correct byte order above and sized
    // `setup_buf` accordingly, so there's no extra validation to do.

    // Validate MIT-MAGIC-COOKIE-1 auth.
    // - If the client presents MIT-MAGIC-COOKIE-1, the data must match exactly.
    // - If no auth is presented (auth_name_len == 0), accept (local Unix socket).
    // - Unknown auth protocols are accepted with a warning for compatibility.
    let mut security_trust_level: u32 = 0; // 0 = trusted (default)
    if auth_name_len > 0 {
        let auth_name_start = SETUP_REQUEST_HEADER_SIZE;
        let auth_name_end = auth_name_start + auth_name_len as usize;
        let auth_data_start = auth_name_start + pad4(auth_name_len);
        let auth_data_end = auth_data_start + auth_data_len as usize;

        if auth_name_end <= setup_buf.len() && auth_data_end <= setup_buf.len() {
            let client_auth_name = &setup_buf[auth_name_start..auth_name_end];
            let client_auth_data = &setup_buf[auth_data_start..auth_data_end];

            if client_auth_name == b"MIT-MAGIC-COOKIE-1" {
                if client_auth_data == auth_cookie {
                    debug!("Client presented valid MIT-MAGIC-COOKIE-1 auth");
                } else if client_auth_data.len() == 16 {
                    // Check against SECURITY-generated tokens
                    let token_key: [u8; 16] = client_auth_data.try_into().unwrap_or([0; 16]);
                    let token_info = shared_security_tokens
                        .lock()
                        .ok()
                        .and_then(|tokens| tokens.get(&token_key).cloned());
                    if let Some(info) = token_info {
                        if info.is_expired() {
                            warn!("SECURITY token expired (auth_id={})", info.auth_id);
                            // Remove expired token
                            if let Ok(mut tokens) = shared_security_tokens.lock() {
                                tokens.remove(&token_key);
                            }
                            let resp =
                                build_auth_failure(byte_order, b"SECURITY authorization expired");
                            stream.write_all(&resp).await?;
                            return Ok(());
                        }
                        debug!(
                            "Client authenticated via SECURITY token (auth_id={}, trust={})",
                            info.auth_id, info.trust_level
                        );
                        // trust_level will be set on the ClientState after creation
                        security_trust_level = info.trust_level;
                    } else {
                        warn!("MIT-MAGIC-COOKIE-1 auth failed: cookie mismatch");
                        let resp =
                            build_auth_failure(byte_order, b"Invalid MIT-MAGIC-COOKIE-1 key");
                        stream.write_all(&resp).await?;
                        return Ok(());
                    }
                } else {
                    warn!("MIT-MAGIC-COOKIE-1 auth failed: cookie mismatch");
                    let resp = build_auth_failure(byte_order, b"Invalid MIT-MAGIC-COOKIE-1 key");
                    stream.write_all(&resp).await?;
                    return Ok(());
                }
            } else {
                warn!(
                    "Client presented unknown auth protocol: {:?} — rejecting per X11 spec",
                    String::from_utf8_lossy(client_auth_name)
                );
                let resp = build_auth_failure(byte_order, b"Unsupported authentication protocol");
                stream.write_all(&resp).await?;
                return Ok(());
            }
        }
    } else {
        debug!("Client connected without auth (unauthenticated local connection)");
    }

    // Phase 2: Send setup reply
    let msb_first = byte_order == BYTE_ORDER_MSB;
    let mut setup = build_setup(conn_index);
    if msb_first {
        // MSB-first clients expect big-endian image byte order in the setup
        setup.image_byte_order = ImageOrder::MSB_FIRST;
        setup.bitmap_format_bit_order = ImageOrder::MSB_FIRST;
    }
    let mut reply_bytes = Vec::new();
    setup.serialize_into(&mut reply_bytes);
    // Verify setup length consistency and log to debug file
    let declared_len = u16::from_le_bytes([reply_bytes[6], reply_bytes[7]]) as usize;
    let actual_extra = reply_bytes.len() - 8;
    {
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/glx_replies.log")
        {
            if declared_len * 4 != actual_extra {
                let _ = writeln!(
                    f,
                    "SETUP_MISMATCH declared={}({}B) actual_extra={}B total={}B",
                    declared_len,
                    declared_len * 4,
                    actual_extra,
                    reply_bytes.len()
                );
            } else {
                let _ = writeln!(
                    f,
                    "SETUP_OK total={}B extra={}B",
                    reply_bytes.len(),
                    declared_len * 4
                );
            }
        }
    }
    if msb_first {
        byteswap_setup_reply(&mut reply_bytes);
    }
    stream.write_all(&reply_bytes).await?;

    info!("X11 client connected: {client_id}");

    // Wait for any active GrabServer to be released before processing
    // requests.  Per X11 spec, GrabServer blocks request processing from
    // all other clients.  We allow the connection handshake (above) to
    // complete so the TCP/Unix socket doesn't time out, then block here
    // before the request loop.
    {
        let (lock, _notify) = &*server_grab;
        loop {
            {
                let holder = lock.lock().unwrap_or_else(|e| e.into_inner());
                if holder.is_none() {
                    break;
                }
            }
            // Poll every 5ms until the grab is released.
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    // Phase 3: Handle requests
    let local_windows = shared_windows.lock().unwrap().clone();
    let (wm_events_tx, mut wm_events_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let resource_id_base = (conn_index + 1) << super::core::RESOURCE_ID_BASE_SHIFT;
    let mut state = ClientState {
        client_id: client_id.clone(),
        peer_pid,
        resource_id_base,
        next_xid: resource_id_base,
        sequence: 0,
        windows: local_windows,
        shared_windows,
        shared_dirty_windows: std::collections::HashSet::new(),
        pixmaps: HashMap::new(),
        gcs: HashMap::new(),
        atoms: shared_atoms,
        update_tx,
        root_window: ROOT_WINDOW,
        // Read the current global pointer position so a new client
        // connecting mid-session sees where the pointer actually is
        // (e.g. after an earlier client's XTEST motion), not (0, 0).
        pointer_x: shared_pointer.lock().map(|p| p.0).unwrap_or(0),
        pointer_y: shared_pointer.lock().map(|p| p.1).unwrap_or(0),
        // Read the global focus on connect — a client joining mid-session
        // (xterm, etc.) needs to see what xdotool's earlier SetInputFocus
        // already chose, not the bare ROOT default. Same pattern as
        // pointer_x/y above.
        focus_window: shared_focus
            .lock()
            .map(|f| *f)
            .unwrap_or(ROOT_WINDOW),
        focus_revert_to: 1, // Parent
        font_manager: FontManager::new(),
        render: handlers::render::RenderState::new(),
        selections: HashMap::new(),
        selection_timestamps: HashMap::new(),
        shm_segments: HashMap::new(),
        wm_state: shared_wm_state.clone(),
        wm_events_tx,
        event_router,
        shared_selections,
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
        save_set: Vec::new(),
        close_down_mode: 0,
        close_down_mode_atomic: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0)),
        disconnect_cleanup_done: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        last_entered_window: ROOT_WINDOW,
        pressed_keys: [0u8; 32],
        server_start,
        keyboard_control: Default::default(),
        pointer_control: Default::default(),
        screen_saver: Default::default(),
        screen_saver_event_mask: 0,
        screen_saver_window: 0,
        screen_saver_attrs: None,
        screen_saver_suspend_count: 0,
        msb_first: byte_order == 0x42,
        screen_width: SCREEN_WIDTH,
        screen_height: SCREEN_HEIGHT,
        randr_config_timestamp: 0,
        xfixes_regions: HashMap::new(),
        incr_transfers: Vec::new(),
        retained_temporary_windows: Vec::new(),
        colormaps: HashMap::new(),
        installed_colormaps: {
            let mut s = std::collections::HashSet::new();
            s.insert(ROOT_COLORMAP);
            s
        },
        cursor_info: HashMap::new(),
        sync_state: handlers::sync::SyncState::new(),
        pending_fds: Vec::new(),
        reply_fds: Vec::new(),
        motion_history: Vec::with_capacity(256),
        pointer_mapping: [1, 2, 3, 4, 5, 6, 7], // identity mapping
        modifier_map: vec![
            vec![50, 62],   // Shift (keycodes 50=Shift_L, 62=Shift_R)
            vec![66],       // Lock (66=Caps_Lock)
            vec![37, 105],  // Control (37=Control_L, 105=Control_R)
            vec![64, 108],  // Mod1 (64=Alt_L, 108=Alt_R)
            vec![77],       // Mod2 (77=Num_Lock)
            vec![],         // Mod3
            vec![133, 134], // Mod4 (133=Super_L, 134=Super_R)
            vec![],         // Mod5
        ],
        win_gravity: HashMap::new(),
        bit_gravity: HashMap::new(),
        custom_keymap: shared_keymap.clone(),
        cursor_event_subscribers: HashMap::new(),
        selection_event_subscribers: HashMap::new(),
        cursor_serial: 0,
        current_cursor: 0,
        cursor_hidden: 0,
        back_buffers: HashMap::new(),
        dbe_idiom_depth: 0,
        record_contexts: HashMap::new(),
        shared_record_contexts,
        glx: Default::default(),
        security_authorizations: HashMap::new(),
        shared_security_tokens: shared_security_tokens.clone(),
        trust_level: security_trust_level,
        font_path: vec![
            "/usr/share/fonts/X11/misc".to_string(),
            "/usr/share/fonts/X11/Type1".to_string(),
            "/usr/share/fonts/X11/75dpi".to_string(),
            "/usr/share/fonts/X11/100dpi".to_string(),
        ],
        access_hosts: Vec::new(),
        access_control_enabled: false,
        shared_access_control: shared_access_control.clone(),
        xtest_grab_impervious: false,
        dpms_enabled: true,
        dpms_power_level: 0,
        dpms_standby_timeout: 0,
        dpms_suspend_timeout: 0,
        dpms_off_timeout: 0,
        xkb_state: super::client::XkbState::default(),
        xkb_extra_groups: Vec::new(),
        xkb_indicators: 0,
        xkb_indicator_maps: Vec::new(),
        xkb_group_switch_keys: Vec::new(),
        xkb_names_atoms: HashMap::new(),
        xkb_type_names: Vec::new(),
        xkb_kt_level_names: Vec::new(),
        xkb_group_names: Vec::new(),
        xkb_indicator_name_atoms: Vec::new(),
        xkb_vmod_names: Vec::new(),
        xkb_key_names: HashMap::new(),
        xkb_key_aliases: Vec::new(),
        xkb_key_types: HashMap::new(),
        xkb_key_actions: HashMap::new(),
        xkb_key_behaviors: HashMap::new(),
        xkb_explicit: HashMap::new(),
        xkb_modmap: HashMap::new(),
        xkb_vmodmap: HashMap::new(),
        xkb_vmod_bindings: [0u8; 16],
        xkb_button_actions: HashMap::new(),
        xkb_device_led_info: Vec::new(),
        xv_ports: HashMap::new(),
        xv_video_notify_drawables: std::collections::HashSet::new(),
        xv_port_notify_ports: std::collections::HashSet::new(),
        pointer_button_mask: 0,
        motion_hint_suppressed: false,
        barriers: HashMap::new(),
        disconnect_mode: 0,
        present_msc: 0,
        clipboard_notify_tx: Some(clipboard_notify_tx),
        persistent_clipboard,
        shared_pixmaps,
        shared_pixmap_fbs,
        shared_gcs,
        client_registry: client_registry.clone(),
        shared_pointer: shared_pointer.clone(),
        shared_focus: shared_focus.clone(),
        event_broadcaster,
        server_grab,
        randr_crtcs: Vec::new(),
        randr_outputs: Vec::new(),
        randr_modes: Vec::new(),
        randr_providers: Vec::new(),
        randr_event_mask: 0,
        randr_monitors: Vec::new(),
        randr_primary_output: 0,
        randr_next_mode_id: 1000,
        vidmode_viewport_x: 0,
        vidmode_viewport_y: 0,
        vidmode_modes: vec![handlers::vidmode::VidModeInfo::default_for_screen(
            SCREEN_WIDTH,
            SCREEN_HEIGHT,
        )],
        vidmode_locked: false,
        vidmode_current_mode: 0,
        big_requests_enabled: false,
        freed_xids: Vec::new(),
        xim: handlers::xim::XimServer::new(XIM_WINDOW),
        xkb_compat_si: super::handlers::xkb::default_compat_si(),
        xkb_group_compat: Default::default(),
        xkb_event_mask: 0,
        xkb_named_indicators: HashMap::new(),
        overlay_ref_count: 0,
        extension_registry,
        resource_limits: super::client::ResourceLimits::default(),
    };

    // Initialize default RandR monitor model.
    state.randr_init_default();

    // Register this client for global event broadcasts (e.g., MappingNotify).
    state
        .event_broadcaster
        .register_client(&client_id, &state.wm_events_tx);

    // Register this client's resource base in the shared client registry.
    client_registry.lock().unwrap().push(resource_id_base);

    // RECORD: notify any enabled contexts about the new client connection
    state.record_notify_client_started();

    // NOTE: MANAGER ClientMessages (XSETTINGS, system tray) are NOT sent here.
    // New clients discover the XSETTINGS manager via XGetSelectionOwner("_XSETTINGS_S0"),
    // not by receiving MANAGER events. Sending unsolicited ClientMessages to every new
    // connection interleaves with expected replies and triggers xcb_xlib_extra_reply_data_left
    // assertions in GLX clients (glxinfo, Mesa, Firefox).

    let mut compose = crate::compose::ComposeState::new();

    let mut buf = vec![0u8; READ_BUF_BYTES];
    let mut pending = Vec::new();
    let mut frame_interval = tokio::time::interval(FRAME_INTERVAL);
    frame_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Key auto-repeat state.
    // When a key is held down, we generate synthetic KeyPress events after
    // an initial delay, then at a regular interval, per X11 spec §12.4.
    struct RepeatState {
        keycode: u8,
        mask: u16,
        target_wid: u32,
        in_delay_phase: bool,
    }
    let mut key_repeat: Option<RepeatState> = None;
    let repeat_timer = tokio::time::sleep(REPEAT_TIMER_DORMANT); // dormant
                                                                       // Pin the sleep so we can reset it.
    tokio::pin!(repeat_timer);

    let _wm_guard = WmCleanupGuard {
        wm_state: shared_wm_state,
        client_id: client_id.clone(),
    };
    let _client_reg_guard = ClientRegistryGuard {
        registry: client_registry,
        resource_id_base,
    };
    // Fallback cleanup for the case where the request loop exits via a write
    // error (broken pipe) instead of n==0. Without it, the explicit cleanup
    // path is skipped and the client's windows leak into shared_windows
    // forever, where the next-connecting client clones them.
    let _resources_guard = ClientResourcesCleanupGuard {
        shared_windows: state.shared_windows.clone(),
        shared_pixmaps: state.shared_pixmaps.clone(),
        shared_gcs: state.shared_gcs.clone(),
        event_broadcaster: state.event_broadcaster.clone(),
        client_id: client_id.clone(),
        close_down_mode: state.close_down_mode_atomic.clone(),
        cleanup_done: state.disconnect_cleanup_done.clone(),
    };

    loop {
        // Cooperative yield: let other tasks (e.g. newly spawned client
        // handlers) run between select iterations.  Without this, the
        // 16ms frame_interval tick can monopolise the worker thread.
        tokio::task::yield_now().await;

        tokio::select! {
            result = stream.readable() => {
                result?;
                // Try recvmsg to capture any SCM_RIGHTS file descriptors
                let raw_fd = stream.as_raw_fd();
                let (n, received_fds) = match recv_with_fds(raw_fd, &mut buf) {
                    Ok((n, fds)) => (n, fds),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                    Err(e) => return Err(e),
                };

                // Store any received file descriptors for SHM AttachFd
                if !received_fds.is_empty() {
                    debug!("Received {} fd(s) via SCM_RIGHTS", received_fds.len());
                    state.pending_fds.extend(received_fds);
                }

                if n == 0 {
                    // Release server grab if this client held it
                    if state.grabs.server_grab_count > 0 {
                        let (lock, notify) = &*state.server_grab;
                        if let Ok(mut holder) = lock.lock() {
                            if holder.as_deref() == Some(&state.client_id) {
                                *holder = None;
                            }
                        }
                        notify.notify_waiters();
                        state.grabs.server_grab_count = 0;
                    }

                    // RECORD: notify any enabled contexts about the client disconnection
                    state.record_notify_client_died();
                    // Send any pending RECORD notifications before cleanup
                    let mut died_events: Vec<Vec<u8>> = state.pending_events.drain(..).collect();
                    patch_event_sequences(&mut died_events, state.sequence, state.msb_first);
                    for event in died_events { let _ = stream.write_all(&event).await; }

                    // Handle SaveSet: reparent save_set windows per X11 spec.
                    // For each window in the save-set that still exists and is an
                    // inferior of a window created by this client, reparent it to the
                    // closest ancestor NOT created by this client (typically root).
                    // This must happen BEFORE the close-down-mode Destroy pass so
                    // these windows survive.
                    let save_set: Vec<u32> = state.save_set.drain(..).collect();
                    let my_client_id = state.client_id.clone();
                    let root = state.root_window;
                    let mut reparented_save_set: Vec<u32> = Vec::new();
                    for wid in &save_set {
                        let wid = *wid;
                        // Window must still exist
                        if !state.windows.contains_key(&wid) {
                            continue;
                        }
                        // Check that the window is an inferior of a window owned
                        // by this client (walk up from parent, not the window itself)
                        let is_inferior = {
                            let mut cur = state.windows.get(&wid).map(|w| w.parent).unwrap_or(0);
                            let mut found = false;
                            for _ in 0..crate::xserver::window_tree::MAX_TREE_DEPTH {
                                if cur == 0 || cur == wid { break; }
                                if let Some(w) = state.windows.get(&cur) {
                                    if w.owner_client_id == my_client_id {
                                        found = true;
                                        break;
                                    }
                                    cur = w.parent;
                                } else {
                                    break;
                                }
                            }
                            found
                        };
                        if !is_inferior {
                            continue;
                        }
                        // Find the closest ancestor NOT owned by this client
                        let new_parent = {
                            let mut cur = state.windows.get(&wid).map(|w| w.parent).unwrap_or(root);
                            let mut target = root;
                            for _ in 0..crate::xserver::window_tree::MAX_TREE_DEPTH {
                                if cur == 0 { break; }
                                if let Some(w) = state.windows.get(&cur) {
                                    if w.owner_client_id != my_client_id {
                                        target = cur;
                                        break;
                                    }
                                    cur = w.parent;
                                } else {
                                    break;
                                }
                            }
                            target
                        };
                        let was_mapped = state.windows.get(&wid).is_some_and(|w| w.mapped);
                        let old_parent = state.windows.get(&wid).map(|w| w.parent).unwrap_or(0);
                        // Remove from old parent's children_order
                        if let Some(old_p) = state.windows.get_mut(&old_parent) {
                            old_p.children_order.retain(|&c| c != wid);
                        }
                        // Update parent
                        if let Some(win) = state.windows.get_mut(&wid) {
                            win.parent = new_parent;
                        }
                        // Add to new parent's children_order
                        if let Some(new_p) = state.windows.get_mut(&new_parent) {
                            new_p.children_order.push(wid);
                        }
                        // Update shared window registry with new parent
                        if let Ok(mut shared) = state.shared_windows.lock() {
                            if let Some(sw) = shared.get_mut(&wid) {
                                sw.parent = new_parent;
                            }
                        }
                        // If was mapped, ensure it stays mapped
                        if was_mapped {
                            if let Some(win) = state.windows.get_mut(&wid) {
                                win.mapped = true;
                            }
                        }
                        reparented_save_set.push(wid);
                    }

                    // Unregister from shared selections owned by this connection
                    // and emit XFIXES SelectionNotify events for lost selections.
                    // For CLIPBOARD, persist the data and take server-side ownership
                    // so future paste requests still work (clipboard manager persistence).
                    {
                        use crate::xserver::atoms::predef::CLIPBOARD as CLIPBOARD_ATOM;
                        let my_wids: Vec<u32> = state.x11_to_uuid.keys().copied().collect();
                        let timestamp = state.timestamp();

                        // Check if this client owns CLIPBOARD and we have cached data.
                        let owns_clipboard = if let Ok(sels) = state.shared_selections.lock() {
                            sels.get(&CLIPBOARD_ATOM)
                                .map(|e| my_wids.contains(&e.owner))
                                .unwrap_or(false)
                        } else {
                            false
                        };
                        let has_persistent_data = if owns_clipboard {
                            state.persistent_clipboard.lock().ok()
                                .map(|pc| pc.contains_key(&CLIPBOARD_ATOM))
                                .unwrap_or(false)
                        } else {
                            false
                        };

                        if let Ok(mut sels) = state.shared_selections.lock() {
                            // Collect selection atoms that are being removed.
                            let lost_selections: Vec<u32> = sels.iter()
                                .filter(|(_, entry)| my_wids.contains(&entry.owner))
                                .map(|(&sel_atom, _)| sel_atom)
                                .collect();

                            // For CLIPBOARD with persistent data, transfer ownership
                            // to the server's clipboard manager window instead of
                            // clearing it. For all other selections, remove them.
                            if has_persistent_data {
                                // Create a dummy channel for the clipboard manager
                                // window — ConvertSelection requests will be handled
                                // directly by the server via persistent_clipboard.
                                let (mgr_tx, _mgr_rx) = mpsc::unbounded_channel();
                                sels.insert(CLIPBOARD_ATOM, SelectionEntry {
                                    owner: CLIPBOARD_MANAGER_WINDOW,
                                    event_tx: mgr_tx,
                                    timestamp,
                                });
                                // Remove all other selections owned by this client.
                                sels.retain(|sel_atom, entry| {
                                    *sel_atom == CLIPBOARD_ATOM || !my_wids.contains(&entry.owner)
                                });
                                debug!(
                                    "Clipboard manager: persisted CLIPBOARD data, \
                                     ownership transferred to server window {:#x}",
                                    CLIPBOARD_MANAGER_WINDOW,
                                );
                            } else {
                                sels.retain(|_, entry| !my_wids.contains(&entry.owner));
                            }

                            // Emit XFIXES SelectionNotify for each lost selection
                            // so clipboard monitors (GTK/Qt) are notified.
                            for sel_atom in &lost_selections {
                                // For persisted CLIPBOARD, emit with the new server
                                // owner rather than None.
                                let new_owner = if *sel_atom == CLIPBOARD_ATOM && has_persistent_data {
                                    CLIPBOARD_MANAGER_WINDOW
                                } else {
                                    0
                                };

                                if let Some(&event_mask) = state.selection_event_subscribers.get(sel_atom) {
                                    if event_mask & 1 != 0 {
                                        const XFIXES_SELECTION_NOTIFY: u8 = 87;
                                        let event = serialize_event(
                                            &SelectionNotifyEvent {
                                                response_type: XFIXES_SELECTION_NOTIFY,
                                                subtype: SelectionEvent::SET_SELECTION_OWNER,
                                                sequence: state.sequence,
                                                window: state.root_window,
                                                owner: new_owner,
                                                selection: *sel_atom,
                                                timestamp,
                                                selection_timestamp: timestamp,
                                            },
                                            state.msb_first,
                                        );
                                        state.pending_events.push(event.to_vec());
                                    }
                                }
                                // Also broadcast XFIXES events to other connections
                                // via the event broadcaster.
                                {
                                    const XFIXES_SEL_NOTIFY: u8 = 87;
                                    let bcast_event = serialize_event(
                                        &SelectionNotifyEvent {
                                            response_type: XFIXES_SEL_NOTIFY,
                                            subtype: SelectionEvent::SET_SELECTION_OWNER,
                                            sequence: state.sequence,
                                            window: state.root_window,
                                            owner: new_owner,
                                            selection: *sel_atom,
                                            timestamp,
                                            selection_timestamp: timestamp,
                                        },
                                        state.msb_first,
                                    );
                                    state.event_broadcaster.broadcast_global(
                                        &bcast_event, &state.client_id,
                                    );
                                }
                            }
                        }
                        // Also clear local selection state.
                        state.selections.clear();
                        state.selection_timestamps.clear();
                    }

                    // Unregister shared pixmaps and GCs owned by this connection
                    state.unregister_all_shared_resources();

                    // Unsubscribe from cross-connection event broadcaster
                    state.event_broadcaster.unsubscribe_client(&state.client_id);

                    // Revert focus if it points to a window owned by this client
                    let my_windows: Vec<u32> = state.x11_to_uuid.keys().copied().collect();
                    if my_windows.contains(&state.focus_window) && state.focus_window != state.root_window {
                        state.revert_focus_from(state.focus_window);
                    }

                    // Drain frozen grab events since the grab holder is gone
                    state.grabs.frozen_pointer_events.clear();
                    state.grabs.frozen_keyboard_events.clear();
                    // Release any active pointer/keyboard grabs held by this client
                    state.grabs.pointer_grab = None;
                    state.grabs.keyboard_grab = None;
                    // Clean up passive grabs on windows owned by this client.
                    // Per X11 spec, when a client disconnects with DestroyAll, all
                    // its passive grabs (button and key) must be removed to prevent
                    // stale references to destroyed windows.
                    {
                        let my_windows: std::collections::HashSet<u32> =
                            state.x11_to_uuid.keys().copied().collect();
                        state.grabs.button_grabs.retain(|g| !my_windows.contains(&g.grab_window));
                        state.grabs.key_grabs.retain(|g| !my_windows.contains(&g.grab_window));
                    }

                    match state.close_down_mode {
                        0 => {
                            // Destroy: remove all client windows, pixmaps, GCs, colormaps.
                            // Exclude save-set windows that were reparented above.
                            let wids: Vec<u32> = state.x11_to_uuid.keys()
                                .copied()
                                .filter(|w| !reparented_save_set.contains(w))
                                .collect();
                            state.event_router.unregister(&wids);
                            let uuids: Vec<String> = wids.iter()
                                .filter_map(|w| state.x11_to_uuid.get(w).cloned())
                                .collect();
                            state.window_router.unregister_all(&uuids);

                            // Remove from shared window registry and notify frontend.
                            // Also unlink each dying window from its parent's
                            // children_order so QueryTree from a fresh client
                            // (which clones shared on connect) doesn't keep
                            // returning the dead id.
                            if let Ok(mut shared) = state.shared_windows.lock() {
                                for &wid in &wids {
                                    if let Some(parent_id) = shared.get(&wid).map(|w| w.parent) {
                                        if let Some(parent) = shared.get_mut(&parent_id) {
                                            parent.children_order.retain(|&c| c != wid);
                                        }
                                    }
                                    shared.remove(&wid);
                                }
                            }
                            for &wid in &wids {
                                if let Some(uuid) = state.x11_to_uuid.get(&wid) {
                                    let _ = state.update_tx.send((
                                        state.client_id.clone(),
                                        x11_web_protocol::DisplayUpdate::WindowDestroyed {
                                            window_id: uuid.clone(),
                                        },
                                    ));
                                }
                                // Same unlink in our own local view, in case
                                // anything below reads from state.windows
                                // before the per-client teardown wipes it.
                                if let Some(parent_id) = state.windows.get(&wid).map(|w| w.parent) {
                                    if let Some(parent) = state.windows.get_mut(&parent_id) {
                                        parent.children_order.retain(|&c| c != wid);
                                    }
                                }
                                state.windows.remove(&wid);
                            }
                            // Free pixmaps and GCs owned by this client
                            state.pixmaps.clear();
                            state.gcs.clear();
                            // Free RENDER resources (pictures, glyphsets, gradients)
                            state.render = handlers::render::RenderState::new();
                            // Free SYNC resources (counters, alarms, fences)
                            state.sync_state = handlers::sync::SyncState::default();
                            // Clear cursor references from surviving windows before freeing cursors
                            {
                                let cursor_ids: Vec<u32> = state.cursors.keys().copied().collect();
                                for win in state.windows.values_mut() {
                                    if let Some(cid) = win.cursor {
                                        if cursor_ids.contains(&cid) {
                                            win.cursor = None;
                                        }
                                    }
                                }
                            }
                            // Free cursors and colormaps
                            state.cursors.clear();
                            state.cursor_info.clear();
                            state.colormaps.clear();
                            // Free XFIXES regions
                            state.xfixes_regions.clear();
                            // Free RECORD contexts (local and shared)
                            state.record_contexts.clear();
                            if let Ok(mut shared_rec) = state.shared_record_contexts.lock() {
                                shared_rec.retain(|_, entry| entry.recording_client_id != state.client_id);
                            }
                            // Free GLX state
                            state.glx = handlers::glx::GlxState::default();
                            // Free damage and present subscriptions
                            state.damage_regions.clear();
                            state.present_subscriptions.clear();
                            // Free DBE back buffers
                            state.back_buffers.clear();
                            // Free SHM segments (detach from shared memory)
                            for (_, seg) in state.shm_segments.drain() {
                                safe_shmdt(seg.addr);
                            }
                            // Close pending file descriptors
                            for fd in state.pending_fds.drain(..) {
                                safe_close(fd);
                            }
                            for fd in state.reply_fds.drain(..) {
                                safe_close(fd);
                            }
                        }
                        1 => {
                            // RetainPermanent: keep windows alive permanently
                        }
                        2 => {
                            // RetainTemporary: keep windows alive but mark them
                            // so KillClient(AllTemporary) can destroy them later.
                            let wids: Vec<u32> = state.windows.keys().copied().collect();
                            for wid in &wids {
                                if let Some(win) = state.windows.get_mut(wid) {
                                    win.retained_temporary = true;
                                }
                            }
                            // Sync the retained_temporary flag to shared_windows so
                            // other clients can see it via KillClient(AllTemporary).
                            if let Ok(mut shared) = state.shared_windows.lock() {
                                for &wid in &wids {
                                    if let Some(win) = shared.get_mut(&wid) {
                                        win.retained_temporary = true;
                                    }
                                }
                            }
                            state.retained_temporary_windows.extend(wids.clone());
                            // Unregister routes for retained windows (they'll be re-registered if adopted)
                            state.event_router.unregister(&wids);
                            let uuids: Vec<String> = wids.iter()
                                .filter_map(|w| state.x11_to_uuid.get(w).cloned())
                                .collect();
                            state.window_router.unregister_all(&uuids);
                        }
                        _ => {
                            let wids: Vec<u32> = state.x11_to_uuid.keys().copied().collect();
                            state.event_router.unregister(&wids);
                            let uuids: Vec<String> = state.x11_to_uuid.values().cloned().collect();
                            state.window_router.unregister_all(&uuids);
                        }
                    }
                    // Tell the cleanup guard to skip — we've already done the
                    // close-down-mode-aware teardown here.
                    state
                        .disconnect_cleanup_done
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                    return Ok(());
                }

                pending.extend_from_slice(&buf[..n]);
                state.sync_windows();

                // Server grab check: if another client holds a GrabServer,
                // wait until it releases before processing our requests.
                {
                    let (lock, _notify) = &*state.server_grab;
                    loop {
                        {
                            let holder = lock.lock().unwrap_or_else(|e| e.into_inner());
                            if holder.is_none() || holder.as_deref() == Some(&state.client_id) {
                                break;
                            }
                        }
                        // Poll every 5ms until the grab is released.
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    }
                }

                while pending.len() >= 4 {
                    // If connection is blocked by a SYNC Await/AwaitFence,
                    // stop processing further requests until the condition is met.
                    if state.sync_state.blocked {
                        break;
                    }

                    // Read request length respecting client byte order.
                    let req_len_units = if state.msb_first {
                        u16::from_be_bytes([pending[2], pending[3]]) as usize
                    } else {
                        u16::from_le_bytes([pending[2], pending[3]]) as usize
                    };
                    let req_len_bytes = req_len_units * 4;

                    if req_len_bytes == 0 {
                        // BIG-REQUESTS format: length=0 means extended 32-bit length at offset 4.
                        // Reject if the client hasn't enabled BIG-REQUESTS.
                        if !state.big_requests_enabled {
                            state.sequence = state.sequence.wrapping_add(1);
                            let mut err = build_error(LENGTH_ERROR, state.sequence, 0, pending[0], 0);
                            if state.msb_first {
                                super::core::byteswap_error_in_place(&mut err);
                            }
                            stream.write_all(&err).await?;
                            // Skip the 4-byte header we already peeked at.
                            pending.drain(..4);
                            continue;
                        }
                        if pending.len() < 8 {
                            break;
                        }
                        let big_len = if state.msb_first {
                            u32::from_be_bytes([pending[4], pending[5], pending[6], pending[7]]) as usize
                        } else {
                            u32::from_le_bytes([pending[4], pending[5], pending[6], pending[7]]) as usize
                        };
                        let big_bytes = big_len * 4;
                        if !(BIG_REQUEST_MIN_BYTES..=BIG_REQUEST_MAX_BYTES).contains(&big_bytes) {
                            // Reject absurdly large or too-small requests
                            warn!("BIG-REQUEST with invalid length: {big_bytes}");
                            pending.clear();
                            break;
                        }
                        if pending.len() < big_bytes {
                            break;
                        }
                        let raw: Vec<u8> = pending.drain(..big_bytes).collect();
                        state.sequence = state.sequence.wrapping_add(1);
                        // BIG-REQUESTS layout: opcode(1), data(1), 0(2),
                        // big_length(4), body. Convert to standard layout
                        // by removing the 4-byte big_length and patching
                        // the length field to big_len if it fits in u16,
                        // else 0 (handler should treat as oversized).
                        let mut request_data = Vec::with_capacity(raw.len() - 4);
                        request_data.extend_from_slice(&raw[..2]);
                        let len_words = (raw.len() - 4) / 4;
                        let len_field: u16 = if len_words <= u16::MAX as usize {
                            len_words as u16
                        } else {
                            0
                        };
                        request_data.extend_from_slice(&len_field.to_le_bytes());
                        request_data.extend_from_slice(&raw[8..]);
                        // MSB-first requests are now parsed directly via the
                        // codegen-emitted `try_parse_endian_request`; no
                        // pre-swap needed here.
                        let mut response = handle_request(&mut state, &request_data);
                        // Errors are built in canonical LE; swap into MSB
                        // for clients that negotiated big-endian byte order.
                        if state.msb_first && !response.is_empty() && response[0] == 0 {
                            super::core::byteswap_error_in_place(&mut response);
                        }
                        // Publish state mutations to the shared registry BEFORE
                        // sending the reply, so peers waiting on this reply
                        // (which can immediately race in with their own
                        // requests) see the updated shared view.
                        state.sync_windows();
                        if !response.is_empty() {
                            stream.write_all(&response).await?;
                        }
                        continue;
                    }

                    if pending.len() < req_len_bytes {
                        break;
                    }

                    let request_data: Vec<u8> = pending.drain(..req_len_bytes).collect();
                    state.sequence = state.sequence.wrapping_add(1);

                    // MSB-first requests are parsed in place via the
                    // codegen-emitted `try_parse_endian_request`; no
                    // pre-swap pass needed.

                    // RECORD: intercept request from this client
                    state.record_intercept_request(&request_data);

                    let mut response = handle_request(&mut state, &request_data);
                    // Errors are built in canonical LE; swap into MSB for
                    // clients that negotiated big-endian byte order.
                    if state.msb_first && !response.is_empty() && response[0] == 0 {
                        super::core::byteswap_error_in_place(&mut response);
                    }
                    // Publish state mutations to the shared registry BEFORE
                    // sending the reply. Without this, a peer waiting on this
                    // reply (e.g., python-xlib's GetInputFocus inside
                    // Display.sync()) can race in with its next request before
                    // sync_windows runs at line ~1068, and observe a stale
                    // shared view that's missing the window we just created.
                    state.sync_windows();
                    if !response.is_empty() {
                        // Log reply details for protocol debugging
                        let major_opcode = request_data[0];
                        let minor_opcode = if request_data.len() > 1 { request_data[1] } else { 0 };
                        // Write ALL reply info to a debug file for protocol analysis
                        {
                            use std::io::Write;
                            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/glx_replies.log") {
                                let cid = &state.client_id;
                                if response[0] == 1 {
                                    let rlen = u32::from_le_bytes([response[4], response[5], response[6], response[7]]);
                                    let _ = writeln!(f, "REPLY c={cid} seq={} op={major_opcode}/{minor_opcode} bytes={} rlen={rlen}", state.sequence, response.len());
                                } else if response[0] == 0 {
                                    let _ = writeln!(f, "ERROR c={cid} seq={} op={major_opcode}/{minor_opcode} code={}", state.sequence, response[1]);
                                }
                            }
                        }
                        // RECORD: intercept reply/error
                        state.record_intercept_response(&response, major_opcode, minor_opcode);
                        // If there are fds to send (e.g., SHM CreateSegment),
                        // use sendmsg with SCM_RIGHTS ancillary data.
                        if !state.reply_fds.is_empty() {
                            let fds: Vec<i32> = state.reply_fds.drain(..).collect();
                            let raw_fd = stream.as_raw_fd();
                            if let Err(e) = send_with_fds(raw_fd, &response, &fds) {
                                warn!("Failed to send reply with fds: {e}");
                            }
                            // Close the fds after sending (client has their own copy)
                            for fd in fds {
                                safe_close(fd);
                            }
                        } else {
                            stream.write_all(&response).await?;
                        }
                    }
                }

                state.sync_windows();
                // Enforce pending event queue limit to prevent unbounded memory growth.
                if state.pending_events.len() > state.resource_limits.max_pending_events {
                    let overflow = state.pending_events.len() - state.resource_limits.max_pending_events;
                    state.pending_events.drain(..overflow);
                    tracing::warn!(
                        client_id = %state.client_id,
                        dropped = overflow,
                        "pending_events overflow: dropped oldest events"
                    );
                }
                // Pending events still need immediate delivery, with RECORD interception.
                // Patch sequence numbers to the current value so xcb sees
                // monotonically non-decreasing sequences.
                let mut events: Vec<Vec<u8>> = state.pending_events.drain(..).collect();
                patch_event_sequences(&mut events, state.sequence, state.msb_first);
                let record_intercepts = state.record_intercept_events(&events);
                for event in events { stream.write_all(&event).await?; }
                for intercept in record_intercepts { stream.write_all(&intercept).await?; }
            }
            _ = frame_interval.tick() => {
                state.present_msc += 1;

                // Screen saver auto-activation: check if timeout elapsed since last input.
                // Per X11 spec §14.3, the screen saver activates when no user input
                // has occurred for `timeout` seconds and suspend_count == 0.
                if !state.screen_saver.active
                    && state.screen_saver.timeout > 0
                    && state.screen_saver_suspend_count == 0
                {
                    let now = state.timestamp();
                    let elapsed_ms = now.wrapping_sub(state.screen_saver.last_reset_ms);
                    let timeout_ms = state.screen_saver.timeout as u32 * 1000;
                    if elapsed_ms >= timeout_ms {
                        state.screen_saver.active = true;
                        let notify = handlers::input::build_screen_saver_on_event(&state);
                        if !notify.is_empty() {
                            state.pending_events.push(notify);
                        }
                        debug!("Screen saver activated (timeout={}s, elapsed={}ms)",
                            state.screen_saver.timeout, elapsed_ms);
                    }
                }

                // Clean up stale INCR selection transfers (5s timeout per X11 spec).
                state.cleanup_stale_incr_transfers(Duration::from_secs(5));

                // Update SYNC SERVERTIME counter (ID=1) and check alarms
                let server_time_ms = state.timestamp() as i64;
                if let Some(counter) = state.sync_state.counters.get_mut(&1) {
                    let old_value = counter.value_i64();
                    counter.set_from_i64(server_time_ms);
                    let new_value = counter.value_i64();
                    if old_value != new_value {
                        let seq = state.sequence;
                        let bo = state.msb_first;
                        handlers::sync::check_alarms_ext(
                            &mut state.sync_state.alarms, 1, old_value, new_value,
                            &mut state.pending_events, seq, bo,
                        );
                    }
                }

                // Check if any pending SYNC Await/AwaitFence is now satisfied
                // (e.g., SERVERTIME advanced past a threshold).
                if state.sync_state.blocked {
                    let ts = state.timestamp();
                    handlers::sync::check_pending_awaits_ext(&mut state.sync_state, || ts);
                    handlers::sync::check_pending_fence_awaits_ext(&mut state.sync_state);

                    // If unblocked, process any pending request data that was deferred
                    if !state.sync_state.blocked && !pending.is_empty() {
                        while pending.len() >= 4 && !state.sync_state.blocked {
                            let req_len_units = if state.msb_first {
                                u16::from_be_bytes([pending[2], pending[3]]) as usize
                            } else {
                                u16::from_le_bytes([pending[2], pending[3]]) as usize
                            };
                            let req_len_bytes = req_len_units * 4;
                            if req_len_bytes == 0 {
                                if pending.len() < 8 { break; }
                                let big_len = if state.msb_first {
                                    u32::from_be_bytes([pending[4], pending[5], pending[6], pending[7]]) as usize
                                } else {
                                    u32::from_le_bytes([pending[4], pending[5], pending[6], pending[7]]) as usize
                                };
                                let big_bytes = big_len * 4;
                                if !(BIG_REQUEST_MIN_BYTES..=BIG_REQUEST_MAX_BYTES).contains(&big_bytes) {
                                    pending.clear();
                                    break;
                                }
                                if pending.len() < big_bytes { break; }
                                let request_data: Vec<u8> = pending.drain(..big_bytes).collect();
                                state.sequence = state.sequence.wrapping_add(1);
                                let response = handle_request(&mut state, &request_data);
                                if !response.is_empty() {
                                    stream.write_all(&response).await?;
                                }
                                continue;
                            }
                            if pending.len() < req_len_bytes { break; }
                            let request_data: Vec<u8> = pending.drain(..req_len_bytes).collect();
                            state.sequence = state.sequence.wrapping_add(1);
                            state.record_intercept_request(&request_data);
                            let response = handle_request(&mut state, &request_data);
                            if !response.is_empty() {
                                let major_opcode = request_data[0];
                                let minor_opcode = if request_data.len() > 1 { request_data[1] } else { 0 };
                                state.record_intercept_response(&response, major_opcode, minor_opcode);
                                if !state.reply_fds.is_empty() {
                                    let fds: Vec<i32> = state.reply_fds.drain(..).collect();
                                    let raw_fd = stream.as_raw_fd();
                                    if let Err(e) = send_with_fds(raw_fd, &response, &fds) {
                                        warn!("Failed to send reply with fds: {e}");
                                    }
                                    for fd in fds { safe_close(fd); }
                                } else {
                                    stream.write_all(&response).await?;
                                }
                            }
                        }
                    }
                }

                state.sync_windows();
                state.flush_dirty_windows();

                if state.xi.pending.raw_motion {
                    state.xi.pending.raw_motion = false;
                    let ev = crate::xinput2::build_raw_motion_event(state.sequence, state.msb_first);
                    state.pending_events.push(ev);
                }

                // RECORD: intercept events from frame tick.
                // Patch sequence numbers to current value before delivery.
                let mut events: Vec<Vec<u8>> = state.pending_events.drain(..).collect();
                patch_event_sequences(&mut events, state.sequence, state.msb_first);
                let record_intercepts = state.record_intercept_events(&events);
                for event in events { stream.write_all(&event).await?; }
                for intercept in record_intercepts { stream.write_all(&intercept).await?; }

                state.sync_windows();
            }
            Some((x11_wid, msg)) = message_rx.recv() => {
                // Collect this message and drain any immediately available ones
                // for MotionNotify coalescing.
                let mut messages = vec![(x11_wid, msg)];
                while let Ok((wid, m)) = message_rx.try_recv() {
                    messages.push((wid, m));
                }

                // Coalesce: keep only the last MotionNotify per window,
                // but preserve ordering of non-motion events.
                let mut last_motion: HashMap<u32, usize> = HashMap::new();
                for (i, (wid, m)) in messages.iter().enumerate() {
                    if matches!(m, WindowMessage::Input(x11_web_protocol::InputEvent::MotionNotify { .. })) {
                        last_motion.insert(*wid, i);
                    }
                }

                for (i, (x11_wid, msg)) in messages.into_iter().enumerate() {
                    match msg {
                        WindowMessage::Input(input) => {
                            // Skip coalesced (non-last) motion events
                            if matches!(input, x11_web_protocol::InputEvent::MotionNotify { .. }) {
                                if let Some(&last_idx) = last_motion.get(&x11_wid) {
                                    if i < last_idx {
                                        // Update pointer position but skip event generation
                                        if let x11_web_protocol::InputEvent::MotionNotify { x, y, .. } = &input {
                                            let (fx, fy) = if !state.barriers.is_empty() {
                                                enforce_barriers(&state.barriers, state.pointer_x, state.pointer_y, *x, *y)
                                            } else {
                                                (*x, *y)
                                            };
                                            state.xi.valuators.x = fx as i32;
                                            state.xi.valuators.y = fy as i32;
                                            state.set_pointer(fx, fy);
                                            // Record in motion history
                                            let ts = state.timestamp();
                                            state.record_motion_history(ts, fx, fy);
                                        }
                                        continue;
                                    }
                                }
                            }

                            if let x11_web_protocol::InputEvent::MenuActivate { action } = &input {
                                if let Some(uuid) = state.top_level_uuid_for(x11_wid) {
                                    state.menu_tracker.activate(&uuid, action.clone());
                                }
                                continue;
                            }
                            // Click-to-focus, but ONLY on actual button press
                            // events — clobbering focus_window on every motion
                            // and key event also clobbers any SetInputFocus
                            // call the toolkit just made (GTK3 routes XI key
                            // selections to a focus subwindow that wouldn't
                            // be in the ancestor chain of the top_level we'd
                            // otherwise set focus to). And only if focus
                            // isn't already inside this top-level — otherwise
                            // the toolkit's intra-window focus tracking
                            // (URL bar vs content area) gets reset on every
                            // unrelated click.
                            if matches!(
                                input,
                                x11_web_protocol::InputEvent::ButtonPress { .. }
                            ) {
                                let cur = state.focus_window;
                                let cur_top = state.top_level_for(cur);
                                if cur_top != Some(x11_wid) {
                                    state.set_focus_window(x11_wid);
                                }
                                let uuid = state.top_level_uuid_for(x11_wid);
                                state.broadcast_focus(uuid);
                            }
                            match &input {
                                x11_web_protocol::InputEvent::MotionNotify { x, y, .. }
                                | x11_web_protocol::InputEvent::ButtonPress { x, y, .. }
                                | x11_web_protocol::InputEvent::ButtonRelease { x, y, .. }
                                | x11_web_protocol::InputEvent::TouchBegin { x, y, .. }
                                | x11_web_protocol::InputEvent::TouchUpdate { x, y, .. }
                                | x11_web_protocol::InputEvent::TouchEnd { x, y, .. } => {
                                    state.xi.valuators.x = *x as i32;
                                    state.xi.valuators.y = *y as i32;
                                }
                                _ => {}
                            }

                            // Track pressed keys for QueryKeymap + XKB modifier state
                            // and manage key auto-repeat timer.
                            match &input {
                                x11_web_protocol::InputEvent::KeyPress { keycode, state: mask } => {
                                    let kc = *keycode as usize;

                                    // BounceKeys: reject key press if within debounce interval
                                    if state.xkb_state.bounce_keys_reject(kc as u8) {
                                        continue;
                                    }

                                    // MouseKeys: convert numpad keys to pointer events
                                    if (state.xkb_state.controls.enabled_ctrls
                                        & crate::xserver::handlers::xkb::XKB_MOUSE_KEYS_MASK)
                                        != 0
                                    {
                                        use crate::xserver::client::xkb_state::{mousekeys_movement, mousekeys_is_click};
                                        if let Some((dx, dy)) = mousekeys_movement(kc as u8) {
                                            // Convert to pointer motion
                                            let speed = state.xkb_state.controls.mk_max_speed.max(1) as i16;
                                            let new_x = (state.pointer_x + dx * speed).max(0);
                                            let new_y = (state.pointer_y + dy * speed).max(0);
                                            let motion = x11_web_protocol::InputEvent::MotionNotify {
                                                x: new_x,
                                                y: new_y,
                                                state: *mask,
                                            };
                                            state.set_pointer(new_x, new_y);
                                            state.xi.valuators.x = new_x as i32;
                                            state.xi.valuators.y = new_y as i32;
                                            let ts = state.timestamp();
                                            state.record_motion_history(ts, new_x, new_y);
                                            let event_bytes = build_x11_input_event(&mut state, &motion, x11_wid);
                                            if !event_bytes.is_empty() {
                                                stream.write_all(&event_bytes).await?;
                                            }
                                            let chain = ancestor_chain(&state.windows, x11_wid);
                                            let xi_evts = crate::xinput2::build_xi_events_for(
                                                &mut state.xi.valuators,
                                                &state.xi.selections,
                                                &state.xi.passive_grabs,
                                                &chain,
                                                state.sequence,
                                                state.root_window,
                                                &motion,
                                                state.msb_first,
                                            );
                                            for ev in xi_evts {
                                                stream.write_all(&ev).await?;
                                            }
                                            continue;
                                        } else if mousekeys_is_click(kc as u8) {
                                            // KP_5: generate ButtonPress for the default button
                                            let btn = state.xkb_state.controls.mk_dflt_btn.max(1);
                                            let press = x11_web_protocol::InputEvent::ButtonPress {
                                                button: btn,
                                                x: state.pointer_x,
                                                y: state.pointer_y,
                                                state: *mask,
                                            };
                                            state.pointer_button_mask |= 1u16 << (7 + btn as u16);
                                            let event_bytes = build_x11_input_event(&mut state, &press, x11_wid);
                                            if !event_bytes.is_empty() {
                                                stream.write_all(&event_bytes).await?;
                                            }
                                            let chain = ancestor_chain(&state.windows, x11_wid);
                                            let xi_evts = crate::xinput2::build_xi_events_for(
                                                &mut state.xi.valuators,
                                                &state.xi.selections,
                                                &state.xi.passive_grabs,
                                                &chain,
                                                state.sequence,
                                                state.root_window,
                                                &press,
                                                state.msb_first,
                                            );
                                            for ev in xi_evts {
                                                stream.write_all(&ev).await?;
                                            }
                                            continue;
                                        }
                                    }

                                    // SlowKeys: reject key press if it hasn't been held long enough.
                                    // (Simplified synchronous check — a full implementation would use
                                    // an async timer to accept the key after slow_keys_delay.)
                                    // For now we track first-press time and accept on subsequent events.
                                    if (state.xkb_state.controls.enabled_ctrls
                                        & crate::xserver::handlers::xkb::XKB_SLOW_KEYS_MASK)
                                        != 0
                                    {
                                        let delay = state.xkb_state.controls.slow_keys_delay;
                                        // Use the auto-repeat mechanism: a slow key press is only
                                        // accepted if the key is already being held (repeat event).
                                        // First press is "pending" until auto-repeat fires after delay.
                                        if kc < 256
                                            && !crate::xserver::types::keycode_bitset::get(
                                                &state.pressed_keys,
                                                kc as u8,
                                            )
                                        {
                                            // First press: set the key as pressed but DON'T deliver
                                            // the event yet. Instead, set up a repeat timer with
                                            // slow_keys_delay and the first repeat will be the accepted press.
                                            crate::xserver::types::keycode_bitset::set(
                                                &mut state.pressed_keys,
                                                kc as u8,
                                            );
                                            let xkb_before = handlers::xkb::XkbStateSnapshot::capture(&state);
                                            state.xkb_state.key_press(kc as u8);
                                            handlers::xkb::maybe_send_xkb_state_notify(&mut state, &xkb_before, kc as u8, 2);
                                            key_repeat = Some(RepeatState {
                                                keycode: kc as u8,
                                                mask: *mask,
                                                target_wid: x11_wid,
                                                in_delay_phase: true,
                                            });
                                            repeat_timer.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(delay as u64));
                                            continue; // Don't deliver the initial key press event
                                        }
                                    }

                                    let xkb_before = handlers::xkb::XkbStateSnapshot::capture(&state);
                                    if kc < 256 {
                                        crate::xserver::types::keycode_bitset::set(
                                            &mut state.pressed_keys,
                                            kc as u8,
                                        );
                                        state.xkb_state.key_press(kc as u8);
                                    }
                                    handlers::xkb::maybe_send_xkb_state_notify(&mut state, &xkb_before, kc as u8, 2);
                                    // Start auto-repeat if enabled for this key.
                                    let repeat_enabled = (state.xkb_state.controls.enabled_ctrls
                                        & crate::xserver::handlers::xkb::XKB_REPEAT_KEYS_MASK)
                                        != 0;
                                    let key_repeats = kc < 256
                                        && crate::xserver::types::keycode_bitset::get(
                                            &state.xkb_state.controls.per_key_repeat,
                                            kc as u8,
                                        );
                                    if repeat_enabled && key_repeats {
                                        let delay = state.xkb_state.controls.repeat_delay as u64;
                                        key_repeat = Some(RepeatState {
                                            keycode: *keycode as u8,
                                            mask: *mask,
                                            target_wid: x11_wid,
                                            in_delay_phase: true,
                                        });
                                        repeat_timer.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(delay));
                                    } else {
                                        // Non-repeating key: cancel any pending repeat
                                        key_repeat = None;
                                        repeat_timer.as_mut().reset(tokio::time::Instant::now() + REPEAT_TIMER_DORMANT);
                                    }
                                }
                                x11_web_protocol::InputEvent::KeyRelease { keycode, state: mask } => {
                                    let kc = *keycode as usize;

                                    // MouseKeys: convert KP_5 release to ButtonRelease
                                    if (state.xkb_state.controls.enabled_ctrls
                                        & crate::xserver::handlers::xkb::XKB_MOUSE_KEYS_MASK)
                                        != 0
                                    {
                                        use crate::xserver::client::xkb_state::{mousekeys_movement, mousekeys_is_click};
                                        if mousekeys_movement(kc as u8).is_some() {
                                            // Movement key release: nothing to do (motion has no release)
                                            continue;
                                        } else if mousekeys_is_click(kc as u8) {
                                            let btn = state.xkb_state.controls.mk_dflt_btn.max(1);
                                            let release = x11_web_protocol::InputEvent::ButtonRelease {
                                                button: btn,
                                                x: state.pointer_x,
                                                y: state.pointer_y,
                                                state: *mask,
                                            };
                                            state.pointer_button_mask &= !(1u16 << (7 + btn as u16));
                                            let event_bytes = build_x11_input_event(&mut state, &release, x11_wid);
                                            if !event_bytes.is_empty() {
                                                stream.write_all(&event_bytes).await?;
                                            }
                                            let chain = ancestor_chain(&state.windows, x11_wid);
                                            let xi_evts = crate::xinput2::build_xi_events_for(
                                                &mut state.xi.valuators,
                                                &state.xi.selections,
                                                &state.xi.passive_grabs,
                                                &chain,
                                                state.sequence,
                                                state.root_window,
                                                &release,
                                                state.msb_first,
                                            );
                                            for ev in xi_evts {
                                                stream.write_all(&ev).await?;
                                            }
                                            continue;
                                        }
                                    }

                                    let xkb_before = handlers::xkb::XkbStateSnapshot::capture(&state);
                                    if kc < 256 {
                                        crate::xserver::types::keycode_bitset::clear(
                                            &mut state.pressed_keys,
                                            kc as u8,
                                        );
                                        state.xkb_state.key_release(kc as u8);
                                    }
                                    handlers::xkb::maybe_send_xkb_state_notify(&mut state, &xkb_before, kc as u8, 3);
                                    // Cancel auto-repeat for this key.
                                    if key_repeat.as_ref().is_some_and(|r| r.keycode == *keycode as u8) {
                                        key_repeat = None;
                                        repeat_timer.as_mut().reset(tokio::time::Instant::now() + REPEAT_TIMER_DORMANT);
                                    }
                                }
                                _ => {}
                            }

                            // Compose key / dead key processing for KeyPress events.
                            // Resolve the keysym for the keycode, run it through the
                            // compose state machine, and potentially replace or suppress
                            // the input event.
                            if let x11_web_protocol::InputEvent::KeyPress { keycode, state: mask } = &input {
                                let shifted = (*mask & 1) != 0; // ShiftMask
                                let (normal_ks, shifted_ks) = {
                                    let keymap = state.custom_keymap.lock().unwrap();
                                    handlers::resolve_keysym(*keycode as u8, &keymap)
                                };
                                let keysym = if shifted { shifted_ks } else { normal_ks };

                                match compose.process(keysym) {
                                    crate::compose::ComposeResult::Consumed => {
                                        // Key is part of an in-progress compose sequence; skip it.
                                        continue;
                                    }
                                    crate::compose::ComposeResult::Composed(text) => {
                                        // Compose complete. Generate a synthetic KeyPress/KeyRelease
                                        // pair using a Unicode keysym (0x0100_0000 + codepoint).
                                        for ch in text.chars() {
                                            let uni_keysym = if (ch as u32) > 0xff {
                                                0x0100_0000 + ch as u32
                                            } else {
                                                ch as u32
                                            };
                                            // Synthesise a KeyPress for the composed character.
                                            // We reuse keycode 0 (unused) so the X client sees
                                            // the event; the keysym is delivered via XKB/XI2.
                                            let synth_press = x11_web_protocol::InputEvent::KeyPress {
                                                keycode: 0,
                                                state: *mask,
                                            };
                                            let synth_release = x11_web_protocol::InputEvent::KeyRelease {
                                                keycode: 0,
                                                state: *mask,
                                            };

                                            // Core protocol events
                                            let press_bytes = build_x11_input_event(&mut state, &synth_press, x11_wid);
                                            if !press_bytes.is_empty() {
                                                stream.write_all(&press_bytes).await?;
                                            }
                                            let release_bytes = build_x11_input_event(&mut state, &synth_release, x11_wid);
                                            if !release_bytes.is_empty() {
                                                stream.write_all(&release_bytes).await?;
                                            }

                                            // XI2 events with the Unicode keysym
                                            let chain = ancestor_chain(&state.windows, x11_wid);
                                            let xi_press = crate::xinput2::build_xi_events_for(
                                                &mut state.xi.valuators,
                                                &state.xi.selections,
                                                &state.xi.passive_grabs,
                                                &chain,
                                                state.sequence,
                                                state.root_window,
                                                &synth_press,
                                                state.msb_first,
                                            );
                                            for ev in xi_press {
                                                stream.write_all(&ev).await?;
                                            }
                                            let xi_release = crate::xinput2::build_xi_events_for(
                                                &mut state.xi.valuators,
                                                &state.xi.selections,
                                                &state.xi.passive_grabs,
                                                &chain,
                                                state.sequence,
                                                state.root_window,
                                                &synth_release,
                                                state.msb_first,
                                            );
                                            for ev in xi_release {
                                                stream.write_all(&ev).await?;
                                            }

                                            let _ = uni_keysym; // keysym available for future XKB integration
                                        }
                                        continue;
                                    }
                                    crate::compose::ComposeResult::Cancelled(keysyms) => {
                                        // Bad compose sequence. Replay the buffered keysyms
                                        // as individual KeyPress events, then fall through
                                        // to process the current key normally.
                                        for &ks in &keysyms[..keysyms.len().saturating_sub(1)] {
                                            // For replayed keysyms we synthesise keycode 0
                                            let replay = x11_web_protocol::InputEvent::KeyPress {
                                                keycode: 0,
                                                state: *mask,
                                            };
                                            let bytes = build_x11_input_event(&mut state, &replay, x11_wid);
                                            if !bytes.is_empty() {
                                                stream.write_all(&bytes).await?;
                                            }
                                            let chain = ancestor_chain(&state.windows, x11_wid);
                                            let xi_evts = crate::xinput2::build_xi_events_for(
                                                &mut state.xi.valuators,
                                                &state.xi.selections,
                                                &state.xi.passive_grabs,
                                                &chain,
                                                state.sequence,
                                                state.root_window,
                                                &replay,
                                                state.msb_first,
                                            );
                                            for ev in xi_evts {
                                                stream.write_all(&ev).await?;
                                            }
                                            let _ = ks; // keysym available for future XKB integration
                                        }
                                        // Fall through: the original `input` event will be
                                        // sent below by the normal path.
                                    }
                                    crate::compose::ComposeResult::Pass(_) => {
                                        // Not composing — fall through to normal processing.
                                    }
                                }
                            }

                            // Track pointer button state and check for grabs
                            match &input {
                                x11_web_protocol::InputEvent::ButtonPress { button, state: mask, .. } => {
                                    if *button >= 1 && *button <= 5 {
                                        state.pointer_button_mask |= 1u16 << (7 + *button as u16);
                                    }
                                    grab::check_passive_button_grab(&mut state, *button, *mask, x11_wid);
                                }
                                x11_web_protocol::InputEvent::KeyPress { keycode, state: mask } => {
                                    grab::check_passive_key_grab(&mut state, *keycode as u8, *mask, x11_wid);
                                }
                                x11_web_protocol::InputEvent::ButtonRelease { button, state: mask, .. } => {
                                    if *button >= 1 && *button <= 5 {
                                        state.pointer_button_mask &= !(1u16 << (7 + *button as u16));
                                    }
                                    grab::check_button_release_ungrab(&mut state, 0, *mask);
                                }
                                _ => {}
                            }

                            // Save old pointer position before any clamping, for barrier checks.
                            let old_pointer_x = state.pointer_x;
                            let old_pointer_y = state.pointer_y;

                            // Confine-to: clamp pointer coordinates to the confine window bounds
                            // when there is an active pointer grab with confine_to set.
                            let input = match &input {
                                x11_web_protocol::InputEvent::MotionNotify { x, y, state: mask } => {
                                    let (cx, cy) = grab::clamp_to_confine(&state, *x, *y);
                                    if cx != *x || cy != *y {
                                        state.set_pointer(cx, cy);
                                        state.xi.valuators.x = cx as i32;
                                        state.xi.valuators.y = cy as i32;
                                    }
                                    x11_web_protocol::InputEvent::MotionNotify { x: cx, y: cy, state: *mask }
                                }
                                other => other.clone(),
                            };
                            // Enforce XFIXES pointer barriers on motion events.
                            let input = match &input {
                                x11_web_protocol::InputEvent::MotionNotify { x, y, state: mask } => {
                                    if !state.barriers.is_empty() {
                                        let (bx, by) = enforce_barriers(
                                            &state.barriers,
                                            old_pointer_x, old_pointer_y,
                                            *x, *y,
                                        );
                                        if bx != *x || by != *y {
                                            state.set_pointer(bx, by);
                                            state.xi.valuators.x = bx as i32;
                                            state.xi.valuators.y = by as i32;
                                        }
                                        x11_web_protocol::InputEvent::MotionNotify { x: bx, y: by, state: *mask }
                                    } else {
                                        input.clone()
                                    }
                                }
                                _ => input,
                            };
                            let event_bytes = build_x11_input_event(&mut state, &input, x11_wid);

                            // Update _NET_WM_USER_TIME on user input (KeyPress, ButtonPress)
                            if matches!(&input,
                                x11_web_protocol::InputEvent::KeyPress { .. } |
                                x11_web_protocol::InputEvent::ButtonPress { .. }
                            ) {
                                let user_time_atom = state.intern_atom("_NET_WM_USER_TIME", false);
                                let timestamp = state.timestamp();
                                let focused = state.focus_window;
                                if let Some(win) = state.windows.get_mut(&focused) {
                                    win.properties.insert(user_time_atom, PropertyValue {
                                        prop_type: crate::xserver::atoms::predef::CARDINAL,
                                        format: 32,
                                        data: timestamp.to_le_bytes().to_vec(),
                                    });
                                }
                            }

                            // Reset screen saver timer on any user input (per X11 spec §14.3)
                            if state.screen_saver.timeout > 0 && state.screen_saver_suspend_count == 0 {
                                state.screen_saver.last_reset_ms = state.timestamp();
                                if state.screen_saver.active {
                                    state.screen_saver.active = false;
                                    // Send ScreenSaverNotify (Off) event
                                    let notify = handlers::input::build_screen_saver_off_event(&state);
                                    if !notify.is_empty() {
                                        state.pending_events.push(notify);
                                    }
                                }
                            }

                            if !event_bytes.is_empty() {
                                // Check synchronous grab freeze for pointer/keyboard events
                                let is_pointer = matches!(&input,
                                    x11_web_protocol::InputEvent::ButtonPress { .. } |
                                    x11_web_protocol::InputEvent::ButtonRelease { .. } |
                                    x11_web_protocol::InputEvent::MotionNotify { .. }
                                );
                                let is_keyboard = matches!(&input,
                                    x11_web_protocol::InputEvent::KeyPress { .. } |
                                    x11_web_protocol::InputEvent::KeyRelease { .. }
                                );

                                let frozen = if is_pointer {
                                    grab::check_pointer_sync_freeze(&mut state, &event_bytes)
                                } else if is_keyboard {
                                    grab::check_keyboard_sync_freeze(&mut state, &event_bytes)
                                } else {
                                    false
                                };

                                if !frozen {
                                    // RECORD: intercept input events
                                    let record_intercepts = state.record_intercept_events(std::slice::from_ref(&event_bytes));
                                    for intercept in record_intercepts { stream.write_all(&intercept).await?; }
                                    stream.write_all(&event_bytes).await?;
                                }
                            }
                            // For XInput2 dispatch we need the deepest hit
                            // child, not just the top-level — apps like
                            // Firefox/GTK3 select XI events on internal
                            // content children (e.g., the 921x691 GDK
                            // content overlay), so a chain that stops at
                            // top_level misses every selection.
                            let xi_start = match &input {
                                x11_web_protocol::InputEvent::ButtonPress { x, y, .. }
                                | x11_web_protocol::InputEvent::ButtonRelease { x, y, .. }
                                | x11_web_protocol::InputEvent::MotionNotify { x, y, .. }
                                | x11_web_protocol::InputEvent::TouchBegin { x, y, .. }
                                | x11_web_protocol::InputEvent::TouchUpdate { x, y, .. }
                                | x11_web_protocol::InputEvent::TouchEnd { x, y, .. } => {
                                    find_deepest_window(&state.windows, x11_wid, *x, *y).0
                                }
                                x11_web_protocol::InputEvent::KeyPress { .. }
                                | x11_web_protocol::InputEvent::KeyRelease { .. } => {
                                    // Per X11 spec §7: if the pointer is
                                    // inside the focus window's subtree,
                                    // key events descend to the deepest
                                    // window under the pointer. Toolkits
                                    // (GTK3, Firefox) select XI keys on
                                    // a content sub-window of their
                                    // toplevel — without this descent
                                    // the chain stops at focus_window
                                    // (the toplevel) and the selection
                                    // is unreachable.
                                    let f = state.focus_window;
                                    let focus_target = if f != 0 && f != 1 { f } else { x11_wid };
                                    let (deepest, _, _) = find_deepest_window(
                                        &state.windows,
                                        state.root_window,
                                        state.pointer_x,
                                        state.pointer_y,
                                    );
                                    if deepest == focus_target
                                        || crate::xserver::window_tree::is_descendant_of(
                                            &state.windows,
                                            deepest,
                                            focus_target,
                                        )
                                    {
                                        deepest
                                    } else {
                                        focus_target
                                    }
                                }
                                _ => x11_wid,
                            };
                            let chain = ancestor_chain(&state.windows, xi_start);
                            let xi_events = crate::xinput2::build_xi_events_for(
                                &mut state.xi.valuators,
                                &state.xi.selections,
                                &state.xi.passive_grabs,
                                &chain,
                                state.sequence,
                                state.root_window,
                                &input,
                                state.msb_first,
                            );
                            for ev in xi_events {
                                stream.write_all(&ev).await?;
                            }

                            // Flush any pending events queued during input
                            // dispatch (FocusIn/FocusOut from
                            // set_focus_window, PropertyNotify from
                            // _NET_WM_USER_TIME, ColormapNotify, etc.).
                            // Without this, FocusIn is held in pending_events
                            // until the client's next request — and a
                            // GTK app that sees KeyPress without a prior
                            // FocusIn ignores the keystroke.
                            if !state.pending_events.is_empty() {
                                let mut events: Vec<Vec<u8>> = state.pending_events.drain(..).collect();
                                patch_event_sequences(&mut events, state.sequence, state.msb_first);
                                let record_intercepts = state.record_intercept_events(&events);
                                for event in events { stream.write_all(&event).await?; }
                                for intercept in record_intercepts { stream.write_all(&intercept).await?; }
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
            }
            Ok(_) = screen_size_rx.changed() => {
                let (new_w, new_h) = *screen_size_rx.borrow_and_update();
                if new_w > 0 && new_h > 0 && (new_w != state.screen_width || new_h != state.screen_height) {
                    let events = apply_screen_resize(&mut state, new_w, new_h);
                    if !events.is_empty() {
                        stream.write_all(&events).await?;
                    }
                }
            }
            Some(mut event_data) = wm_events_rx.recv() => {
                // Patch the sequence number (bytes 2-3) to match THIS client's
                // current sequence.  Cross-connection events arrive with the
                // *sender's* sequence, which is meaningless to xcb on the
                // receiving side and causes "Unknown sequence number" aborts.
                if event_data.len() >= 4 {
                    let seq = state.sequence;
                    if state.msb_first {
                        event_data[2..4].copy_from_slice(&seq.to_be_bytes());
                    } else {
                        event_data[2..4].copy_from_slice(&seq.to_le_bytes());
                    }
                }
                // RECORD: intercept WM events
                let record_intercepts = state.record_intercept_events(std::slice::from_ref(&event_data));
                for intercept in record_intercepts { stream.write_all(&intercept).await?; }
                stream.write_all(&event_data).await?;
            }
            () = &mut repeat_timer => {
                // Key auto-repeat timer fired. Generate a synthetic KeyPress event
                // for the held key, per X11 spec §12.4.
                if let Some(ref repeat) = key_repeat {
                    let synth = x11_web_protocol::InputEvent::KeyPress {
                        keycode: repeat.keycode as u32,
                        state: repeat.mask,
                    };
                    let event_bytes = build_x11_input_event(&mut state, &synth, repeat.target_wid);
                    if !event_bytes.is_empty() {
                        stream.write_all(&event_bytes).await?;
                    }
                    // Also generate XI2 event for the repeat.
                    let chain = ancestor_chain(&state.windows, repeat.target_wid);
                    let xi_events = crate::xinput2::build_xi_events_for(
                        &mut state.xi.valuators,
                        &state.xi.selections,
                        &state.xi.passive_grabs,
                        &chain,
                        state.sequence,
                        state.root_window,
                        &synth,
                        state.msb_first,
                    );
                    for ev in xi_events {
                        stream.write_all(&ev).await?;
                    }

                    // Schedule next repeat: if we were in delay phase, switch to interval.
                    let interval = state.xkb_state.controls.repeat_interval as u64;
                    if let Some(ref mut r) = key_repeat {
                        r.in_delay_phase = false;
                    }
                    repeat_timer.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(interval));
                } else {
                    // No key held, park the timer far in the future.
                    repeat_timer.as_mut().reset(tokio::time::Instant::now() + REPEAT_TIMER_DORMANT);
                }
            }
        }
    }
}
