/// Helper macro to create a glyph array inline
macro_rules! glyph {
    ($($b:expr),+ $(,)?) => { [$($b),+] };
}

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::sync::mpsc;
use tracing::{debug, error, info};
use x11rb_protocol::protocol::xproto::*;
use x11rb_protocol::x11_utils::{Serialize, TryParse};

use tokio::sync::broadcast;
use uuid::Uuid;
use x11_web_protocol::{DisplayUpdate, InputEvent};

/// A display update tagged with the client_id that produced it.
pub type TaggedDisplayUpdate = (String, DisplayUpdate);

/// Minimal X11 server that accepts client connections and translates
/// X11 drawing operations into DisplayUpdate messages.
pub struct X11Server {
    display_number: u32,
    socket_path: PathBuf,
    update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
    input_tx: broadcast::Sender<(String, InputEvent)>,
    resize_tx: broadcast::Sender<(String, u16, u16)>,
    client_connected_tx: mpsc::UnboundedSender<String>,
}

/// Per-connection state for an X11 client.
struct ClientState {
    client_id: String,
    sequence: u16,
    windows: HashMap<u32, WindowState>,
    pixmaps: HashMap<u32, PixmapState>,
    gcs: HashMap<u32, GcState>,
    atoms: AtomManager,
    update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
    root_window: u32,
    root_width: u16,
    root_height: u16,
    pointer_x: i16,
    pointer_y: i16,
}

#[derive(Clone)]
struct WindowState {
    id: u32,
    parent: u32,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
    border_width: u16,
    visual: u32,
    class: u16,
    mapped: bool,
    event_mask: u32,
    background_pixel: u32,
    override_redirect: bool,
}

#[derive(Clone)]
struct PixmapState {
    _id: u32,
    _width: u16,
    _height: u16,
    _depth: u8,
}

#[derive(Clone)]
struct GcState {
    foreground: u32,
    background: u32,
    line_width: u16,
    function: u8,
}

impl Default for GcState {
    fn default() -> Self {
        Self {
            foreground: 0x00_00_00, // black
            background: 0xFF_FF_FF, // white
            line_width: 0,
            function: 3, // GXcopy
        }
    }
}

struct AtomManager {
    atoms: HashMap<String, u32>,
    reverse: HashMap<u32, String>,
    next_atom: u32,
}

impl AtomManager {
    fn new() -> Self {
        let mut mgr = Self {
            atoms: HashMap::new(),
            reverse: HashMap::new(),
            next_atom: 1,
        };
        // Pre-register some standard atoms
        for (name, id) in PREDEFINED_ATOMS {
            mgr.atoms.insert(name.to_string(), *id);
            mgr.reverse.insert(*id, name.to_string());
            if *id >= mgr.next_atom {
                mgr.next_atom = *id + 1;
            }
        }
        mgr
    }

    fn intern(&mut self, name: &str, only_if_exists: bool) -> u32 {
        if let Some(&id) = self.atoms.get(name) {
            return id;
        }
        if only_if_exists {
            return 0;
        }
        let id = self.next_atom;
        self.next_atom += 1;
        self.atoms.insert(name.to_string(), id);
        self.reverse.insert(id, name.to_string());
        id
    }

    fn get_name(&self, atom: u32) -> Option<&str> {
        self.reverse.get(&atom).map(|s| s.as_str())
    }
}

const PREDEFINED_ATOMS: &[(&str, u32)] = &[
    ("PRIMARY", 1),
    ("SECONDARY", 2),
    ("ARC", 3),
    ("ATOM", 4),
    ("BITMAP", 5),
    ("CARDINAL", 6),
    ("COLORMAP", 7),
    ("CURSOR", 8),
    ("CUT_BUFFER0", 9),
    ("CUT_BUFFER1", 10),
    ("CUT_BUFFER2", 11),
    ("CUT_BUFFER3", 12),
    ("CUT_BUFFER4", 13),
    ("CUT_BUFFER5", 14),
    ("CUT_BUFFER6", 15),
    ("CUT_BUFFER7", 16),
    ("DRAWABLE", 17),
    ("FONT", 18),
    ("INTEGER", 19),
    ("PIXMAP", 20),
    ("POINT", 21),
    ("RECTANGLE", 22),
    ("RESOURCE_MANAGER", 23),
    ("RGB_COLOR_MAP", 24),
    ("RGB_BEST_MAP", 25),
    ("RGB_BLUE_MAP", 26),
    ("RGB_DEFAULT_MAP", 27),
    ("RGB_GRAY_MAP", 28),
    ("RGB_GREEN_MAP", 29),
    ("RGB_RED_MAP", 30),
    ("STRING", 31),
    ("VISUALID", 32),
    ("WINDOW", 33),
    ("WM_COMMAND", 34),
    ("WM_HINTS", 35),
    ("WM_CLIENT_MACHINE", 36),
    ("WM_ICON_NAME", 37),
    ("WM_ICON_SIZE", 38),
    ("WM_NAME", 39),
    ("WM_NORMAL_HINTS", 40),
    ("WM_SIZE_HINTS", 41),
    ("WM_ZOOM_HINTS", 42),
    ("MIN_SPACE", 43),
    ("NORM_SPACE", 44),
    ("MAX_SPACE", 45),
    ("END_SPACE", 46),
    ("SUPERSCRIPT_X", 47),
    ("SUPERSCRIPT_Y", 48),
    ("SUBSCRIPT_X", 49),
    ("SUBSCRIPT_Y", 50),
    ("UNDERLINE_POSITION", 51),
    ("UNDERLINE_THICKNESS", 52),
    ("STRIKEOUT_ASCENT", 53),
    ("STRIKEOUT_DESCENT", 54),
    ("ITALIC_ANGLE", 55),
    ("X_HEIGHT", 56),
    ("QUAD_WIDTH", 57),
    ("WEIGHT", 58),
    ("POINT_SIZE", 59),
    ("RESOLUTION", 60),
    ("COPYRIGHT", 61),
    ("NOTICE", 62),
    ("FONT_NAME", 63),
    ("FAMILY_NAME", 64),
    ("FULL_NAME", 65),
    ("CAP_HEIGHT", 66),
    ("WM_CLASS", 67),
    ("WM_TRANSIENT_FOR", 68),
];

impl X11Server {
    pub fn new(
        display_number: u32,
        update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
        input_tx: broadcast::Sender<(String, InputEvent)>,
        resize_tx: broadcast::Sender<(String, u16, u16)>,
        client_connected_tx: mpsc::UnboundedSender<String>,
    ) -> Self {
        let socket_path = PathBuf::from(format!("/tmp/.X11-unix/X{display_number}"));
        Self {
            display_number,
            socket_path,
            update_tx,
            input_tx,
            resize_tx,
            client_connected_tx,
        }
    }

    pub fn display_string(&self) -> String {
        format!(":{}", self.display_number)
    }

    pub async fn run(self) -> io::Result<()> {
        // Ensure socket directory exists
        let dir = self.socket_path.parent().unwrap();
        tokio::fs::create_dir_all(dir).await.ok();

        // Remove stale socket
        tokio::fs::remove_file(&self.socket_path).await.ok();

        let listener = UnixListener::bind(&self.socket_path)?;
        info!(
            "X11 server listening on {} (DISPLAY={})",
            self.socket_path.display(),
            self.display_string()
        );

        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let client_id = Uuid::new_v4().to_string();
                    let update_tx = self.update_tx.clone();
                    let input_rx = self.input_tx.subscribe();
                    let resize_rx = self.resize_tx.subscribe();
                    let _ = self.client_connected_tx.send(client_id.clone());
                    let cid = client_id.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            handle_client(stream, client_id, update_tx, input_rx, resize_rx).await
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

const ROOT_WINDOW: u32 = 0x00000062; // arbitrary root window ID
const ROOT_VISUAL: u32 = 0x00000021;
const ROOT_COLORMAP: u32 = 0x00000020;
const SCREEN_WIDTH: u16 = 1024;
const SCREEN_HEIGHT: u16 = 768;

fn build_setup() -> Setup {
    let visual = Visualtype {
        visual_id: ROOT_VISUAL,
        class: VisualClass::TRUE_COLOR,
        bits_per_rgb_value: 8,
        colormap_entries: 256,
        red_mask: 0x00FF0000,
        green_mask: 0x0000FF00,
        blue_mask: 0x000000FF,
    };

    let depth = Depth {
        depth: 24,
        visuals: vec![visual],
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
        allowed_depths: vec![depth],
    };

    let format24 = Format {
        depth: 24,
        bits_per_pixel: 32,
        scanline_pad: 32,
    };
    let format1 = Format {
        depth: 1,
        bits_per_pixel: 1,
        scanline_pad: 32,
    };

    // Build initial setup - length will be computed after serialization
    let mut setup = Setup {
        status: 1,
        protocol_major_version: 11,
        protocol_minor_version: 0,
        length: 0, // will fix below
        release_number: 0,
        resource_id_base: 0x04000000,
        resource_id_mask: 0x001FFFFF,
        motion_buffer_size: 256,
        maximum_request_length: 65535,
        image_byte_order: ImageOrder::LSB_FIRST,
        bitmap_format_bit_order: ImageOrder::LSB_FIRST,
        bitmap_format_scanline_unit: 32,
        bitmap_format_scanline_pad: 32,
        min_keycode: 8,
        max_keycode: 255,
        vendor: b"x11-web".to_vec(),
        pixmap_formats: vec![format1, format24],
        roots: vec![screen],
    };

    // Compute length: serialize, subtract 8 bytes header, divide by 4
    let mut bytes = Vec::new();
    setup.serialize_into(&mut bytes);
    setup.length = ((bytes.len() - 8) / 4) as u16;

    setup
}

async fn handle_client(
    mut stream: tokio::net::UnixStream,
    client_id: String,
    update_tx: mpsc::UnboundedSender<TaggedDisplayUpdate>,
    mut input_rx: broadcast::Receiver<(String, InputEvent)>,
    mut resize_rx: broadcast::Receiver<(String, u16, u16)>,
) -> io::Result<()> {
    // Phase 1: Read client setup request
    // Read at least 12 bytes for the header
    let mut header_buf = [0u8; 12];
    stream.read_exact(&mut header_buf).await?;

    let byte_order = header_buf[0];
    if byte_order != 0x6c && byte_order != 0x42 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid byte order: 0x{:02x}", byte_order),
        ));
    }

    // Parse auth lengths from the header
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

    // Calculate total setup request size
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

    // We don't validate auth - just accept everything
    let _setup_request = SetupRequest::try_parse(&setup_buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Bad setup: {e:?}")))?;

    // Phase 2: Send setup reply
    let setup = build_setup();
    let mut reply_bytes = Vec::new();
    setup.serialize_into(&mut reply_bytes);
    stream.write_all(&reply_bytes).await?;

    info!("X11 client connected: {client_id}");

    // Phase 3: Handle requests
    let mut state = ClientState {
        client_id: client_id.clone(),
        sequence: 0,
        windows: HashMap::new(),
        pixmaps: HashMap::new(),
        gcs: HashMap::new(),
        atoms: AtomManager::new(),
        update_tx,
        root_window: ROOT_WINDOW,
        root_width: SCREEN_WIDTH,
        root_height: SCREEN_HEIGHT,
        pointer_x: 0,
        pointer_y: 0,
    };

    // Add root window to state
    state.windows.insert(
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
            class: 1, // InputOutput
            mapped: true,
            event_mask: 0,
            background_pixel: 0x00000000,
            override_redirect: false,
        },
    );

    let mut buf = vec![0u8; 256 * 1024]; // 256KB read buffer
    let mut pending = Vec::new(); // Partial request data

    loop {
        tokio::select! {
            result = stream.read(&mut buf) => {
                let n = result?;
                if n == 0 {
                    return Ok(()); // Client disconnected
                }

                pending.extend_from_slice(&buf[..n]);

                // Process complete requests from the pending buffer
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
            }
            result = input_rx.recv() => {
                if let Ok((target_id, input)) = result {
                    if target_id == client_id {
                        let event_bytes = build_x11_input_event(&mut state, &input);
                        if !event_bytes.is_empty() {
                            stream.write_all(&event_bytes).await?;
                        }
                    }
                }
            }
            result = resize_rx.recv() => {
                if let Ok((target_id, width, height)) = result {
                    if target_id == client_id {
                        let events = resize_all_windows(&mut state, width, height);
                        if !events.is_empty() {
                            stream.write_all(&events).await?;
                        }
                    }
                }
            }
        }
    }
}

/// Resize all mapped windows for this client and send ConfigureNotify + Expose events.
fn resize_all_windows(state: &mut ClientState, width: u16, height: u16) -> Vec<u8> {
    let mut events = Vec::new();
    let seq = state.sequence;

    // Update root window dimensions
    state.root_width = width;
    state.root_height = height;

    // Collect window IDs to resize (avoid borrow issues)
    let window_ids: Vec<u32> = state.windows.keys().copied().collect();

    for wid in window_ids {
        if let Some(win) = state.windows.get_mut(&wid) {
            win.width = width;
            win.height = height;

            // Send ConfigureNotify
            let mut event = [0u8; 32];
            event[0] = CONFIGURE_NOTIFY_EVENT;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[4..8].copy_from_slice(&wid.to_le_bytes());
            event[8..12].copy_from_slice(&wid.to_le_bytes());
            event[16..18].copy_from_slice(&win.x.to_le_bytes());
            event[18..20].copy_from_slice(&win.y.to_le_bytes());
            event[20..22].copy_from_slice(&width.to_le_bytes());
            event[22..24].copy_from_slice(&height.to_le_bytes());
            event[24..26].copy_from_slice(&win.border_width.to_le_bytes());
            events.extend_from_slice(&event);

            // Send Expose to trigger redraw
            if win.mapped {
                let mut expose = [0u8; 32];
                expose[0] = EXPOSE_EVENT;
                expose[2..4].copy_from_slice(&seq.to_le_bytes());
                expose[4..8].copy_from_slice(&wid.to_le_bytes());
                expose[12..14].copy_from_slice(&width.to_le_bytes());
                expose[14..16].copy_from_slice(&height.to_le_bytes());
                events.extend_from_slice(&expose);

                // Send display update for the resize
                let _ = state.update_tx.send((
                    state.client_id.clone(),
                    DisplayUpdate::WindowConfigured {
                        window_id: wid,
                        x: win.x,
                        y: win.y,
                        width,
                        height,
                    },
                ));
            }
        }
    }

    events
}

/// Convert a frontend InputEvent into X11 wire-format event bytes (32 bytes).
fn build_x11_input_event(state: &mut ClientState, input: &InputEvent) -> Vec<u8> {
    // Find the topmost mapped window to deliver events to
    let target_window = state
        .windows
        .values()
        .filter(|w| w.mapped && w.id != state.root_window)
        .max_by_key(|w| w.id) // Pick the last created mapped window
        .map(|w| w.id)
        .unwrap_or(state.root_window);

    // Update tracked pointer position for QueryPointer
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

    match input {
        InputEvent::MotionNotify { x, y, state: mask } => {
            event[0] = MOTION_NOTIFY_EVENT; // 6
            event[1] = 0; // detail: Normal
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            // time (4 bytes at offset 4) - use 0
            event[8..12].copy_from_slice(&state.root_window.to_le_bytes()); // root
            event[12..16].copy_from_slice(&target_window.to_le_bytes()); // event window
            event[16..20].copy_from_slice(&target_window.to_le_bytes()); // child
            event[20..22].copy_from_slice(&x.to_le_bytes()); // root_x
            event[22..24].copy_from_slice(&y.to_le_bytes()); // root_y
            event[24..26].copy_from_slice(&x.to_le_bytes()); // event_x
            event[26..28].copy_from_slice(&y.to_le_bytes()); // event_y
            event[28..30].copy_from_slice(&mask.to_le_bytes()); // state
            event[30] = 1; // same_screen
        }
        InputEvent::ButtonPress {
            button,
            x,
            y,
            state: mask,
        } => {
            event[0] = BUTTON_PRESS_EVENT; // 4
            event[1] = *button;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[8..12].copy_from_slice(&state.root_window.to_le_bytes());
            event[12..16].copy_from_slice(&target_window.to_le_bytes());
            event[16..20].copy_from_slice(&target_window.to_le_bytes());
            event[20..22].copy_from_slice(&x.to_le_bytes());
            event[22..24].copy_from_slice(&y.to_le_bytes());
            event[24..26].copy_from_slice(&x.to_le_bytes());
            event[26..28].copy_from_slice(&y.to_le_bytes());
            event[28..30].copy_from_slice(&mask.to_le_bytes());
            event[30] = 1;
        }
        InputEvent::ButtonRelease {
            button,
            x,
            y,
            state: mask,
        } => {
            event[0] = BUTTON_RELEASE_EVENT; // 5
            event[1] = *button;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[8..12].copy_from_slice(&state.root_window.to_le_bytes());
            event[12..16].copy_from_slice(&target_window.to_le_bytes());
            event[16..20].copy_from_slice(&target_window.to_le_bytes());
            event[20..22].copy_from_slice(&x.to_le_bytes());
            event[22..24].copy_from_slice(&y.to_le_bytes());
            event[24..26].copy_from_slice(&x.to_le_bytes());
            event[26..28].copy_from_slice(&y.to_le_bytes());
            event[28..30].copy_from_slice(&mask.to_le_bytes());
            event[30] = 1;
        }
        InputEvent::KeyPress {
            keycode,
            state: mask,
        } => {
            event[0] = KEY_PRESS_EVENT; // 2
            event[1] = *keycode as u8;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[8..12].copy_from_slice(&state.root_window.to_le_bytes());
            event[12..16].copy_from_slice(&target_window.to_le_bytes());
            event[16..20].copy_from_slice(&target_window.to_le_bytes());
            event[28..30].copy_from_slice(&mask.to_le_bytes());
            event[30] = 1;
        }
        InputEvent::KeyRelease {
            keycode,
            state: mask,
        } => {
            event[0] = KEY_RELEASE_EVENT; // 3
            event[1] = *keycode as u8;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[8..12].copy_from_slice(&state.root_window.to_le_bytes());
            event[12..16].copy_from_slice(&target_window.to_le_bytes());
            event[16..20].copy_from_slice(&target_window.to_le_bytes());
            event[28..30].copy_from_slice(&mask.to_le_bytes());
            event[30] = 1;
        }
    }

    event.to_vec()
}

fn handle_request(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let major_opcode = data[0];
    let _minor = data[1];
    let seq = state.sequence;

    match major_opcode {
        1 => handle_create_window(state, data, seq),
        2 => handle_change_window_attributes(state, data),
        3 => handle_get_window_attributes(state, data, seq),
        4 => handle_destroy_window(state, data),
        8 => handle_map_window(state, data, seq),
        9 => handle_map_subwindows(state, data, seq),
        10 => handle_unmap_window(state, data, seq),
        12 => handle_configure_window(state, data, seq),
        14 => handle_get_geometry(state, data, seq),
        15 => handle_query_tree(state, data, seq),
        16 => handle_intern_atom(state, data, seq),
        17 => handle_get_atom_name(state, data, seq),
        18 => handle_change_property(state, data),
        20 => handle_get_property(state, data, seq),
        23 => handle_get_selection_owner(state, data, seq),
        38 => handle_query_pointer(state, data, seq),
        42 => handle_set_input_focus(state, data),
        43 => handle_get_input_focus(state, data, seq),
        47 => handle_query_font(state, data, seq),
        49 => handle_list_fonts(state, data, seq),
        55 => handle_create_gc(state, data),
        56 => handle_change_gc(state, data),
        60 => handle_free_gc(state, data),
        53 => handle_create_pixmap(state, data),
        54 => handle_free_pixmap(state, data),
        61 => handle_clear_area(state, data, seq),
        62 => handle_copy_area(state, data),
        70 => handle_poly_fill_rectangle(state, data),
        65 => handle_poly_line(state, data),
        64 => handle_poly_point(state, data),
        66 => handle_poly_segment(state, data),
        67 => handle_poly_rectangle(state, data),
        68 => handle_poly_arc(state, data),
        69 => handle_fill_poly(state, data),
        71 => handle_poly_fill_arc(state, data),
        72 => handle_put_image(state, data),
        73 => handle_get_image(state, data, seq),
        84 => handle_alloc_color(state, data, seq),
        91 => handle_query_colors(state, data, seq),
        98 => handle_query_extension(state, data, seq),
        // Silently ignore these common requests
        19 | // DeleteProperty
        22 | // SetSelectionOwner
        24 | // ConvertSelection
        25 | // SendEvent
        26 | // GrabPointer -> reply needed
        28 | // UngrabButton
        30 | // ChangeActivePointerGrab
        31 | // UngrabPointer
        33 | // UngrabKeyboard
        35 | // AllowEvents
        36 | // GrabServer
        37 | // UngrabServer
        40 | // TranslateCoordinates -> reply needed
        41 | // WarpPointer
        44 | // QueryKeymap -> reply needed
        45 | // OpenFont
        46 | // CloseFont
        48 | // QueryTextExtents -> reply needed
        50 | // ListFontsWithInfo -> reply needed
        51 | // SetFontPath
        52 | // GetFontPath -> reply needed
        57 | // CopyGC
        58 | // SetDashes
        59 | // SetClipRectangles
        74 | // PolyText8
        75 | // PolyText16
        76 => handle_image_text8(state, data),
        77 | // ImageText16
        78 | // CreateColormap
        79 | // FreeColormap
        88 | // FreeColors
        93 | // CreateCursor
        94 | // CreateGlyphCursor
        95 | // FreeCursor
        96 | // RecolorCursor
        100 | // ChangeKeyboardMapping
        101 | // GetKeyboardMapping -> reply needed
        102 | // ChangeKeyboardControl
        103 | // GetKeyboardControl -> reply needed
        104 | // Bell
        115 | // ForceScreenSaver
        116 | // SetPointerMapping -> reply needed
        119 | // GetModifierMapping -> reply needed
        127 // NoOperation
        => handle_misc_request(state, major_opcode, seq),
        _ => {
            debug!("Unhandled X11 request opcode: {major_opcode}");
            Vec::new()
        }
    }
}

// Handles requests that need stub replies
fn handle_misc_request(state: &ClientState, opcode: u8, seq: u16) -> Vec<u8> {
    match opcode {
        26 => {
            // GrabPointer reply: Success
            let mut reply = [0u8; 32];
            reply[0] = 1; // Reply
            reply[1] = 0; // Success status
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        40 => {
            // TranslateCoordinates reply
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = 1; // same_screen
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[8..12].copy_from_slice(&state.root_window.to_le_bytes()); // child
            reply.to_vec()
        }
        44 => {
            // QueryKeymap reply: all zeros (no keys pressed)
            let mut reply = [0u8; 40]; // 32 + 8 bytes of keymap
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&2u32.to_le_bytes()); // length = 2 (8 extra bytes)
            reply.to_vec()
        }
        48 => {
            // QueryTextExtents reply
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            // font_ascent = 12, font_descent = 4, overall_width = 0
            reply[8..10].copy_from_slice(&12i16.to_le_bytes());
            reply[10..12].copy_from_slice(&4i16.to_le_bytes());
            reply.to_vec()
        }
        52 => {
            // GetFontPath reply: empty list
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        // 84 (AllocColor) and 91 (QueryColors) handled in handle_request
        101 => {
            // GetKeyboardMapping reply
            // Return a minimal mapping: 248 keycodes, 1 keysym per keycode
            let keysyms_per_keycode: u8 = 1;
            let num_keycodes: u32 = 248; // max_keycode(255) - min_keycode(8) + 1
            let data_len = num_keycodes * keysyms_per_keycode as u32;
            let reply_len = 32 + data_len as usize * 4;
            let mut reply = vec![0u8; reply_len];
            reply[0] = 1;
            reply[1] = keysyms_per_keycode;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&data_len.to_le_bytes());
            reply
        }
        103 => {
            // GetKeyboardControl reply
            let mut reply = [0u8; 52]; // 32 + 20 extra
            reply[0] = 1;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&5u32.to_le_bytes()); // length = 5 (20 extra bytes)
            reply.to_vec()
        }
        116 => {
            // SetPointerMapping reply: Success
            let mut reply = [0u8; 32];
            reply[0] = 1;
            reply[1] = 0; // MappingSuccess
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply.to_vec()
        }
        119 => {
            // GetModifierMapping reply
            let keycodes_per_modifier: u8 = 2;
            let data_len = 8 * keycodes_per_modifier as u32; // 8 modifiers
            let reply_len = 32 + data_len as usize;
            let mut reply = vec![0u8; reply_len];
            reply[0] = 1;
            reply[1] = keycodes_per_modifier;
            reply[2..4].copy_from_slice(&seq.to_le_bytes());
            reply[4..8].copy_from_slice(&((data_len / 4).to_le_bytes()));
            reply
        }
        _ => Vec::new(),
    }
}

fn handle_create_window(state: &mut ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() < 32 {
        return Vec::new();
    }

    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let parent = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let x = i16::from_le_bytes([data[12], data[13]]);
    let y = i16::from_le_bytes([data[14], data[15]]);
    let width = u16::from_le_bytes([data[16], data[17]]);
    let height = u16::from_le_bytes([data[18], data[19]]);
    let border_width = u16::from_le_bytes([data[20], data[21]]);
    let class = u16::from_le_bytes([data[22], data[23]]);
    let visual = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
    let value_mask = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);

    let mut background_pixel = 0u32;
    let mut event_mask = 0u32;
    let mut override_redirect = false;

    // Parse value list
    let mut offset = 32;
    for bit in 0..15 {
        if value_mask & (1 << bit) != 0 {
            if offset + 4 <= data.len() {
                let val = u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]);
                match bit {
                    0 => {} // background-pixmap
                    1 => background_pixel = val,
                    2 => {} // border-pixmap
                    3 => {} // border-pixel
                    4 => {} // bit-gravity
                    5 => {} // win-gravity
                    6 => {} // backing-store
                    7 => {} // backing-planes
                    8 => {} // backing-pixel
                    9 => override_redirect = val != 0,
                    10 => {} // save-under
                    11 => event_mask = val,
                    12 => {} // do-not-propagate-mask
                    13 => {} // colormap
                    14 => {} // cursor
                    _ => {}
                }
                offset += 4;
            }
        }
    }

    let use_visual = if visual == 0 { ROOT_VISUAL } else { visual };

    debug!("CreateWindow: id={wid:#x} parent={parent:#x} {x},{y} {width}x{height}");

    state.windows.insert(
        wid,
        WindowState {
            id: wid,
            parent,
            x,
            y,
            width,
            height,
            border_width,
            visual: use_visual,
            class,
            mapped: false,
            event_mask,
            background_pixel,
            override_redirect,
        },
    );

    let _ = state.update_tx.send((
        state.client_id.clone(),
        DisplayUpdate::WindowCreated {
            window_id: wid,
            x,
            y,
            width,
            height,
        },
    ));

    Vec::new() // No reply for CreateWindow
}

fn handle_change_window_attributes(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let value_mask = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

    if let Some(win) = state.windows.get_mut(&wid) {
        let mut offset = 12;
        for bit in 0..15 {
            if value_mask & (1 << bit) != 0 {
                if offset + 4 <= data.len() {
                    let val = u32::from_le_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]);
                    match bit {
                        1 => win.background_pixel = val,
                        11 => win.event_mask = val,
                        _ => {}
                    }
                    offset += 4;
                }
            }
        }
    }

    Vec::new()
}

fn handle_get_window_attributes(state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    let mut reply = vec![0u8; 44];
    reply[0] = 1; // Reply
    reply[1] = 0; // backing-store: NotUseful
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&3u32.to_le_bytes()); // length = 3 extra u32s

    if let Some(win) = state.windows.get(&wid) {
        reply[8..12].copy_from_slice(&win.visual.to_le_bytes());
        reply[12..14].copy_from_slice(&win.class.to_le_bytes());
        // bit_gravity = 0, win_gravity = 0
        reply[18..22].copy_from_slice(&0u32.to_le_bytes()); // backing_planes
        reply[22..26].copy_from_slice(&0u32.to_le_bytes()); // backing_pixel
        reply[26] = 0; // save_under = false
        reply[27] = 1; // map_is_installed = true
        reply[28] = if win.mapped { 2 } else { 0 }; // map_state: Viewable or Unmapped
        reply[29] = if win.override_redirect { 1 } else { 0 };
        reply[30..34].copy_from_slice(&ROOT_COLORMAP.to_le_bytes());
        reply[34..38].copy_from_slice(&win.event_mask.to_le_bytes());
        reply[38..42].copy_from_slice(&0u32.to_le_bytes()); // your_event_mask
        reply[42..44].copy_from_slice(&0u16.to_le_bytes()); // do_not_propagate_mask
    }

    reply
}

fn handle_destroy_window(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    state.windows.remove(&wid);
    let _ = state.update_tx.send((
        state.client_id.clone(),
        DisplayUpdate::WindowDestroyed { window_id: wid },
    ));
    Vec::new()
}

fn handle_map_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    let mut events = Vec::new();

    if let Some(win) = state.windows.get_mut(&wid) {
        win.mapped = true;
        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::WindowMapped { window_id: wid },
        ));

        // Fill window with its background pixel (like a real X server does)
        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::FillRect {
                window_id: wid,
                x: 0,
                y: 0,
                width: win.width,
                height: win.height,
                color: win.background_pixel,
            },
        ));

        // Send MapNotify event
        let mut map_event = [0u8; 32];
        map_event[0] = MAP_NOTIFY_EVENT;
        map_event[2..4].copy_from_slice(&seq.to_le_bytes());
        map_event[4..8].copy_from_slice(&wid.to_le_bytes()); // event window
        map_event[8..12].copy_from_slice(&wid.to_le_bytes()); // window
        map_event[12] = if win.override_redirect { 1 } else { 0 };
        events.extend_from_slice(&map_event);

        // Send Expose event
        let width = win.width;
        let height = win.height;
        let mut expose_event = [0u8; 32];
        expose_event[0] = EXPOSE_EVENT;
        expose_event[2..4].copy_from_slice(&seq.to_le_bytes());
        expose_event[4..8].copy_from_slice(&wid.to_le_bytes());
        // x=0, y=0 already zero
        expose_event[12..14].copy_from_slice(&width.to_le_bytes());
        expose_event[14..16].copy_from_slice(&height.to_le_bytes());
        // count = 0
        events.extend_from_slice(&expose_event);
    }

    events
}

fn handle_map_subwindows(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let parent = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    // Collect child window IDs first to avoid borrow issues
    let children: Vec<u32> = state
        .windows
        .values()
        .filter(|w| w.parent == parent && !w.mapped)
        .map(|w| w.id)
        .collect();

    let mut all_events = Vec::new();
    for child_id in children {
        // Construct a fake MapWindow request for each child
        let mut fake_data = [0u8; 8];
        fake_data[0] = 8; // MapWindow opcode
        fake_data[2..4].copy_from_slice(&2u16.to_le_bytes()); // length = 2
        fake_data[4..8].copy_from_slice(&child_id.to_le_bytes());
        let events = handle_map_window(state, &fake_data, seq);
        all_events.extend(events);
    }

    all_events
}

fn handle_unmap_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    let mut events = Vec::new();

    if let Some(win) = state.windows.get_mut(&wid) {
        win.mapped = false;
        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::WindowUnmapped { window_id: wid },
        ));

        let mut event = [0u8; 32];
        event[0] = UNMAP_NOTIFY_EVENT;
        event[2..4].copy_from_slice(&seq.to_le_bytes());
        event[4..8].copy_from_slice(&wid.to_le_bytes());
        event[8..12].copy_from_slice(&wid.to_le_bytes());
        events.extend_from_slice(&event);
    }

    events
}

fn handle_configure_window(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let value_mask = u16::from_le_bytes([data[8], data[9]]);

    let mut offset = 12;
    let mut changed = false;

    if let Some(win) = state.windows.get_mut(&wid) {
        for bit in 0..7 {
            if value_mask & (1 << bit) != 0 {
                if offset + 4 <= data.len() {
                    let val = u32::from_le_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]);
                    match bit {
                        0 => {
                            win.x = val as i16;
                            changed = true;
                        }
                        1 => {
                            win.y = val as i16;
                            changed = true;
                        }
                        2 => {
                            win.width = val as u16;
                            changed = true;
                        }
                        3 => {
                            win.height = val as u16;
                            changed = true;
                        }
                        4 => {
                            win.border_width = val as u16;
                        }
                        5 => {} // sibling
                        6 => {} // stack-mode
                        _ => {}
                    }
                    offset += 4;
                }
            }
        }

        if changed {
            let _ = state.update_tx.send((
                state.client_id.clone(),
                DisplayUpdate::WindowConfigured {
                    window_id: wid,
                    x: win.x,
                    y: win.y,
                    width: win.width,
                    height: win.height,
                },
            ));

            // Send ConfigureNotify
            let mut event = [0u8; 32];
            event[0] = CONFIGURE_NOTIFY_EVENT;
            event[2..4].copy_from_slice(&seq.to_le_bytes());
            event[4..8].copy_from_slice(&wid.to_le_bytes()); // event
            event[8..12].copy_from_slice(&wid.to_le_bytes()); // window
                                                              // above_sibling = 0
            event[16..18].copy_from_slice(&win.x.to_le_bytes());
            event[18..20].copy_from_slice(&win.y.to_le_bytes());
            event[20..22].copy_from_slice(&win.width.to_le_bytes());
            event[22..24].copy_from_slice(&win.height.to_le_bytes());
            event[24..26].copy_from_slice(&win.border_width.to_le_bytes());
            event[26] = if win.override_redirect { 1 } else { 0 };
            return event.to_vec();
        }
    }

    Vec::new()
}

fn handle_get_geometry(state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    let mut reply = [0u8; 32];
    reply[0] = 1; // Reply
    reply[1] = 24; // depth
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    // length = 0

    if let Some(win) = state.windows.get(&drawable) {
        reply[8..12].copy_from_slice(&state.root_window.to_le_bytes());
        reply[12..14].copy_from_slice(&win.x.to_le_bytes());
        reply[14..16].copy_from_slice(&win.y.to_le_bytes());
        reply[16..18].copy_from_slice(&win.width.to_le_bytes());
        reply[18..20].copy_from_slice(&win.height.to_le_bytes());
        reply[20..22].copy_from_slice(&win.border_width.to_le_bytes());
    } else {
        reply[8..12].copy_from_slice(&state.root_window.to_le_bytes());
        reply[16..18].copy_from_slice(&state.root_width.to_le_bytes());
        reply[18..20].copy_from_slice(&state.root_height.to_le_bytes());
    }

    reply.to_vec()
}

fn handle_query_tree(state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    let children: Vec<u32> = state
        .windows
        .values()
        .filter(|w| w.parent == wid)
        .map(|w| w.id)
        .collect();

    let n_children = children.len() as u16;
    let reply_len = 32 + children.len() * 4;
    let mut reply = vec![0u8; reply_len];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&(children.len() as u32).to_le_bytes());
    reply[8..12].copy_from_slice(&state.root_window.to_le_bytes());

    let parent = state.windows.get(&wid).map(|w| w.parent).unwrap_or(0);
    reply[12..16].copy_from_slice(&parent.to_le_bytes());
    reply[16..18].copy_from_slice(&n_children.to_le_bytes());

    for (i, &child) in children.iter().enumerate() {
        let off = 32 + i * 4;
        reply[off..off + 4].copy_from_slice(&child.to_le_bytes());
    }

    reply
}

fn handle_intern_atom(state: &mut ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let only_if_exists = data[1] != 0;
    let name_len = u16::from_le_bytes([data[4], data[5]]) as usize;

    let name = if 8 + name_len <= data.len() {
        String::from_utf8_lossy(&data[8..8 + name_len]).to_string()
    } else {
        String::new()
    };

    let atom = state.atoms.intern(&name, only_if_exists);

    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..12].copy_from_slice(&atom.to_le_bytes());

    reply.to_vec()
}

fn handle_get_atom_name(state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    let atom = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    let name = state.atoms.get_name(atom).unwrap_or("");
    let name_bytes = name.as_bytes();
    let padded_len = (name_bytes.len() + 3) & !3;

    let mut reply = vec![0u8; 32 + padded_len];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((padded_len / 4) as u32).to_le_bytes());
    reply[8..10].copy_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    reply[32..32 + name_bytes.len()].copy_from_slice(name_bytes);

    reply
}

fn handle_change_property(_state: &mut ClientState, _data: &[u8]) -> Vec<u8> {
    Vec::new() // No reply
}

fn handle_get_property(_state: &ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    // Return "property not found"
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    // type = 0 (None), format = 0, bytes_after = 0, value_length = 0
    reply.to_vec()
}

fn handle_get_selection_owner(_state: &ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    // owner = None (0)
    reply.to_vec()
}

fn handle_query_pointer(state: &ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 1; // same_screen
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..12].copy_from_slice(&state.root_window.to_le_bytes()); // root
                                                                    // child = 0
    reply[16..18].copy_from_slice(&state.pointer_x.to_le_bytes()); // root_x
    reply[18..20].copy_from_slice(&state.pointer_y.to_le_bytes()); // root_y
    reply[20..22].copy_from_slice(&state.pointer_x.to_le_bytes()); // win_x
    reply[22..24].copy_from_slice(&state.pointer_y.to_le_bytes()); // win_y
                                                                   // mask = 0
    reply.to_vec()
}

fn handle_set_input_focus(_state: &mut ClientState, _data: &[u8]) -> Vec<u8> {
    Vec::new()
}

fn handle_get_input_focus(state: &ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[1] = 1; // revert_to = Parent
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[8..12].copy_from_slice(&state.root_window.to_le_bytes());
    reply.to_vec()
}

fn handle_query_font(_state: &ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    // Return a minimal font info reply
    // QueryFont reply: 60 bytes fixed + properties + char_infos
    let mut reply = vec![0u8; 60];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&7u32.to_le_bytes()); // length (extra data in 4-byte units)
                                                      // min_bounds (12 bytes at offset 8)
                                                      // max_bounds (12 bytes at offset 24)
    reply[24..26].copy_from_slice(&8u16.to_le_bytes()); // character_width
    reply[26..28].copy_from_slice(&12u16.to_le_bytes()); // ascent
    reply[28..30].copy_from_slice(&4u16.to_le_bytes()); // descent
                                                        // min_char_or_byte2 at offset 40
    reply[40..42].copy_from_slice(&32u16.to_le_bytes());
    // max_char_or_byte2 at offset 42
    reply[42..44].copy_from_slice(&126u16.to_le_bytes());
    // default_char at offset 44
    reply[44..46].copy_from_slice(&32u16.to_le_bytes());
    // properties_count at offset 46
    reply[46..48].copy_from_slice(&0u16.to_le_bytes());
    // draw_direction at offset 48
    reply[48] = 0; // LeftToRight
                   // font_ascent at offset 52
    reply[52..54].copy_from_slice(&12i16.to_le_bytes());
    // font_descent at offset 54
    reply[54..56].copy_from_slice(&4i16.to_le_bytes());
    // n_char_infos at offset 56
    reply[56..60].copy_from_slice(&0u32.to_le_bytes());
    reply
}

fn handle_list_fonts(_state: &ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    // Return a single font: "fixed"
    let font_name = b"fixed";
    let str_len = 1 + font_name.len(); // length byte + name
    let padded = (str_len + 3) & !3;

    let mut reply = vec![0u8; 32 + padded];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&((padded / 4) as u32).to_le_bytes());
    reply[8..10].copy_from_slice(&1u16.to_le_bytes()); // number of names
    reply[32] = font_name.len() as u8;
    reply[33..33 + font_name.len()].copy_from_slice(font_name);

    reply
}

fn handle_create_gc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let gc_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let _drawable = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let value_mask = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

    let mut gc = GcState::default();
    parse_gc_values(&mut gc, value_mask, &data[16..]);
    state.gcs.insert(gc_id, gc);

    Vec::new()
}

fn handle_change_gc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let gc_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let value_mask = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

    if let Some(gc) = state.gcs.get_mut(&gc_id) {
        parse_gc_values(gc, value_mask, &data[12..]);
    }

    Vec::new()
}

fn parse_gc_values(gc: &mut GcState, value_mask: u32, data: &[u8]) {
    let mut offset = 0;
    for bit in 0..23 {
        if value_mask & (1 << bit) != 0 {
            if offset + 4 <= data.len() {
                let val = u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]);
                match bit {
                    0 => gc.function = val as u8,
                    2 => gc.foreground = val,
                    3 => gc.background = val,
                    5 => gc.line_width = val as u16,
                    _ => {} // Ignore other GC attributes for now
                }
                offset += 4;
            }
        }
    }
}

fn handle_free_gc(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let gc_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    state.gcs.remove(&gc_id);
    Vec::new()
}

fn handle_create_pixmap(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let depth = data[1];
    let pid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let _drawable = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let width = u16::from_le_bytes([data[12], data[13]]);
    let height = u16::from_le_bytes([data[14], data[15]]);

    state.pixmaps.insert(
        pid,
        PixmapState {
            _id: pid,
            _width: width,
            _height: height,
            _depth: depth,
        },
    );

    Vec::new()
}

fn handle_free_pixmap(state: &mut ClientState, data: &[u8]) -> Vec<u8> {
    let pid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    state.pixmaps.remove(&pid);
    Vec::new()
}

fn handle_clear_area(state: &ClientState, data: &[u8], _seq: u16) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }

    let wid = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let x = i16::from_le_bytes([data[8], data[9]]);
    let y = i16::from_le_bytes([data[10], data[11]]);
    let mut width = u16::from_le_bytes([data[12], data[13]]);
    let mut height = u16::from_le_bytes([data[14], data[15]]);

    // If width or height is 0, use the window's dimensions
    // ClearArea fills with the window's background pixel
    let bg = state.windows.get(&wid).map(|w| {
        if width == 0 {
            width = w.width;
        }
        if height == 0 {
            height = w.height;
        }
        w.background_pixel
    });

    // Send as a FillRect with the background color (more useful than ClearArea
    // for the frontend renderer, which otherwise clears to transparent/black)
    if let Some(bg_pixel) = bg {
        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::FillRect {
                window_id: wid,
                x,
                y,
                width,
                height,
                color: bg_pixel,
            },
        ));
    } else {
        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::ClearArea {
                window_id: wid,
                x,
                y,
                width,
                height,
            },
        ));
    }

    Vec::new()
}

fn handle_copy_area(state: &ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 28 {
        return Vec::new();
    }

    let src = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let dst = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let _gc = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let src_x = i16::from_le_bytes([data[16], data[17]]);
    let src_y = i16::from_le_bytes([data[18], data[19]]);
    let dst_x = i16::from_le_bytes([data[20], data[21]]);
    let dst_y = i16::from_le_bytes([data[22], data[23]]);
    let width = u16::from_le_bytes([data[24], data[25]]);
    let height = u16::from_le_bytes([data[26], data[27]]);

    let _ = state.update_tx.send((
        state.client_id.clone(),
        DisplayUpdate::CopyArea {
            src_window_id: src,
            dst_window_id: dst,
            src_x,
            src_y,
            dst_x,
            dst_y,
            width,
            height,
        },
    ));

    Vec::new()
}

fn handle_poly_rectangle(state: &ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut offset = 12;
    while offset + 8 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let width = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let height = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);

        // Draw as outline using DrawLines (4 edges)
        let x2 = x + width as i16;
        let y2 = y + height as i16;
        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::DrawLines {
                window_id: drawable,
                points: vec![(x, y), (x2, y), (x2, y2), (x, y2), (x, y)],
                color: gc.foreground,
                line_width: gc.line_width,
            },
        ));

        offset += 8;
    }

    Vec::new()
}

fn handle_fill_poly(state: &ClientState, data: &[u8]) -> Vec<u8> {
    // FillPoly: opcode 69
    // [opcode(1), unused(1), length(2), drawable(4), gc(4), shape(1), coord_mode(1), pad(2), points...]
    if data.len() < 16 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut points = Vec::new();
    let mut offset = 16;
    while offset + 4 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        points.push((x, y));
        offset += 4;
    }

    if points.len() >= 3 {
        // Close the polygon
        if points.first() != points.last() {
            points.push(points[0]);
        }
        // Send as filled via DrawLines (frontend could interpret closed polygon as fill)
        // For now, draw as outline which is better than nothing
        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::DrawLines {
                window_id: drawable,
                points,
                color: gc.foreground,
                line_width: gc.line_width,
            },
        ));
    }

    Vec::new()
}

fn handle_poly_fill_rectangle(state: &ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut offset = 12;
    while offset + 8 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let width = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let height = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);

        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::FillRect {
                window_id: drawable,
                x,
                y,
                width,
                height,
                color: gc.foreground,
            },
        ));

        offset += 8;
    }

    Vec::new()
}

fn handle_poly_line(state: &ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let _coord_mode = data[1]; // 0 = Origin, 1 = Previous
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut points = Vec::new();
    let mut offset = 12;
    while offset + 4 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        points.push((x, y));
        offset += 4;
    }

    if !points.is_empty() {
        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::DrawLines {
                window_id: drawable,
                points,
                color: gc.foreground,
                line_width: gc.line_width,
            },
        ));
    }

    Vec::new()
}

fn handle_poly_point(state: &ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut offset = 12;
    while offset + 4 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);

        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::FillRect {
                window_id: drawable,
                x,
                y,
                width: 1,
                height: 1,
                color: gc.foreground,
            },
        ));

        offset += 4;
    }

    Vec::new()
}

fn handle_poly_segment(state: &ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut offset = 12;
    while offset + 8 <= data.len() {
        let x1 = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y1 = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let x2 = i16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let y2 = i16::from_le_bytes([data[offset + 6], data[offset + 7]]);

        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::DrawLines {
                window_id: drawable,
                points: vec![(x1, y1), (x2, y2)],
                color: gc.foreground,
                line_width: gc.line_width,
            },
        ));

        offset += 8;
    }

    Vec::new()
}

fn handle_poly_arc(state: &ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut offset = 12;
    while offset + 12 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let width = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let height = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
        let angle1 = i16::from_le_bytes([data[offset + 8], data[offset + 9]]);
        let angle2 = i16::from_le_bytes([data[offset + 10], data[offset + 11]]);

        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::DrawArc {
                window_id: drawable,
                x,
                y,
                width,
                height,
                angle1,
                angle2,
                filled: false,
                color: gc.foreground,
            },
        ));

        offset += 12;
    }

    Vec::new()
}

fn handle_poly_fill_arc(state: &ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 12 {
        return Vec::new();
    }

    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let mut offset = 12;
    while offset + 12 <= data.len() {
        let x = i16::from_le_bytes([data[offset], data[offset + 1]]);
        let y = i16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let width = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let height = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
        let angle1 = i16::from_le_bytes([data[offset + 8], data[offset + 9]]);
        let angle2 = i16::from_le_bytes([data[offset + 10], data[offset + 11]]);

        let _ = state.update_tx.send((
            state.client_id.clone(),
            DisplayUpdate::DrawArc {
                window_id: drawable,
                x,
                y,
                width,
                height,
                angle1,
                angle2,
                filled: true,
                color: gc.foreground,
            },
        ));

        offset += 12;
    }

    Vec::new()
}

fn handle_image_text8(state: &ClientState, data: &[u8]) -> Vec<u8> {
    // ImageText8: [opcode(1), string_len(1), length(2), drawable(4), gc(4), x(2), y(2), string...]
    if data.len() < 16 {
        return Vec::new();
    }

    let str_len = data[1] as usize;
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let gc_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let x = i16::from_le_bytes([data[12], data[13]]);
    let y = i16::from_le_bytes([data[14], data[15]]);
    let gc = state.gcs.get(&gc_id).cloned().unwrap_or_default();

    let text = if 16 + str_len <= data.len() {
        &data[16..16 + str_len]
    } else {
        return Vec::new();
    };

    let char_w = 7u16;
    let char_h = 13u16;
    let ascent = 10i16;
    let img_w = str_len as u16 * char_w;
    let img_h = char_h;
    let img_y = y - ascent;

    // Render text to a pixel buffer (BGRX format, 4 bytes per pixel)
    let fg_r = ((gc.foreground >> 16) & 0xFF) as u8;
    let fg_g = ((gc.foreground >> 8) & 0xFF) as u8;
    let fg_b = (gc.foreground & 0xFF) as u8;
    let bg_r = ((gc.background >> 16) & 0xFF) as u8;
    let bg_g = ((gc.background >> 8) & 0xFF) as u8;
    let bg_b = (gc.background & 0xFF) as u8;

    let mut pixels = vec![0u8; img_w as usize * img_h as usize * 4];

    // Fill background
    for i in 0..(img_w as usize * img_h as usize) {
        pixels[i * 4] = bg_b;
        pixels[i * 4 + 1] = bg_g;
        pixels[i * 4 + 2] = bg_r;
        pixels[i * 4 + 3] = 0;
    }

    // Render each character using built-in 7x13 bitmap font
    for (ci, &ch) in text.iter().enumerate() {
        let glyph = get_glyph(ch);
        for (row, &bits) in glyph.iter().enumerate() {
            for col in 0..char_w as usize {
                if bits & (1 << (char_w as usize - 1 - col)) != 0 {
                    let px = ci * char_w as usize + col;
                    let py = row;
                    if px < img_w as usize && py < img_h as usize {
                        let idx = (py * img_w as usize + px) * 4;
                        pixels[idx] = fg_b;
                        pixels[idx + 1] = fg_g;
                        pixels[idx + 2] = fg_r;
                        pixels[idx + 3] = 0;
                    }
                }
            }
        }
    }

    let _ = state.update_tx.send((
        state.client_id.clone(),
        DisplayUpdate::PutImage {
            window_id: drawable,
            x,
            y: img_y,
            width: img_w,
            height: img_h,
            data: pixels,
        },
    ));

    Vec::new()
}

/// Simple 7x13 bitmap font glyphs for ASCII printable characters.
/// Each glyph is 13 rows of u8, where each bit represents a pixel (MSB = leftmost).
fn get_glyph(ch: u8) -> &'static [u8; 13] {
    static SPACE: [u8; 13] = [0; 13];
    static DEFAULT: [u8; 13] = [
        0b0111110, 0b1000010, 0b1000010, 0b1000010, 0b0111110, 0b0000000, 0b0000000, 0b0000000,
        0b0000000, 0b0000000, 0b0000000, 0b0000000, 0b0000000,
    ];

    match ch {
        b' ' => &SPACE,
        b'!' => &glyph![
            0b0010000, 0b0010000, 0b0010000, 0b0010000, 0b0010000, 0b0010000, 0b0010000, 0b0000000,
            0b0010000, 0b0000000, 0b0000000, 0b0000000, 0b0000000
        ],
        b'0' => &glyph![
            0b0111100, 0b0100010, 0b1000010, 0b1000010, 0b1000010, 0b1000010, 0b1000010, 0b1000010,
            0b0100010, 0b0111100, 0b0000000, 0b0000000, 0b0000000
        ],
        b'1' => &glyph![
            0b0001000, 0b0011000, 0b0101000, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000,
            0b0001000, 0b0111110, 0b0000000, 0b0000000, 0b0000000
        ],
        b'A'..=b'Z' => {
            static UPPER: [[u8; 13]; 26] = [
                glyph![
                    0b0011100, 0b0100010, 0b1000001, 0b1000001, 0b1111111, 0b1000001, 0b1000001,
                    0b1000001, 0b1000001, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // A
                glyph![
                    0b1111110, 0b1000001, 0b1000001, 0b1111110, 0b1000001, 0b1000001, 0b1000001,
                    0b1000001, 0b1111110, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // B
                glyph![
                    0b0111110, 0b1000001, 0b1000000, 0b1000000, 0b1000000, 0b1000000, 0b1000000,
                    0b1000001, 0b0111110, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // C
                glyph![
                    0b1111100, 0b1000010, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b1000001,
                    0b1000010, 0b1111100, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // D
                glyph![
                    0b1111111, 0b1000000, 0b1000000, 0b1111110, 0b1000000, 0b1000000, 0b1000000,
                    0b1000000, 0b1111111, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // E
                glyph![
                    0b1111111, 0b1000000, 0b1000000, 0b1111110, 0b1000000, 0b1000000, 0b1000000,
                    0b1000000, 0b1000000, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // F
                glyph![
                    0b0111110, 0b1000001, 0b1000000, 0b1000000, 0b1001111, 0b1000001, 0b1000001,
                    0b1000001, 0b0111110, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // G
                glyph![
                    0b1000001, 0b1000001, 0b1000001, 0b1111111, 0b1000001, 0b1000001, 0b1000001,
                    0b1000001, 0b1000001, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // H
                glyph![
                    0b0111110, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000,
                    0b0001000, 0b0111110, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // I
                glyph![
                    0b0011111, 0b0000100, 0b0000100, 0b0000100, 0b0000100, 0b0000100, 0b1000100,
                    0b1000100, 0b0111000, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // J
                glyph![
                    0b1000010, 0b1000100, 0b1001000, 0b1010000, 0b1100000, 0b1010000, 0b1001000,
                    0b1000100, 0b1000010, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // K
                glyph![
                    0b1000000, 0b1000000, 0b1000000, 0b1000000, 0b1000000, 0b1000000, 0b1000000,
                    0b1000000, 0b1111111, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // L
                glyph![
                    0b1000001, 0b1100011, 0b1010101, 0b1001001, 0b1000001, 0b1000001, 0b1000001,
                    0b1000001, 0b1000001, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // M
                glyph![
                    0b1000001, 0b1100001, 0b1010001, 0b1001001, 0b1000101, 0b1000011, 0b1000001,
                    0b1000001, 0b1000001, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // N
                glyph![
                    0b0111110, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b1000001,
                    0b1000001, 0b0111110, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // O
                glyph![
                    0b1111110, 0b1000001, 0b1000001, 0b1000001, 0b1111110, 0b1000000, 0b1000000,
                    0b1000000, 0b1000000, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // P
                glyph![
                    0b0111110, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b1001001, 0b1000101,
                    0b1000010, 0b0111101, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // Q
                glyph![
                    0b1111110, 0b1000001, 0b1000001, 0b1000001, 0b1111110, 0b1001000, 0b1000100,
                    0b1000010, 0b1000001, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // R
                glyph![
                    0b0111110, 0b1000001, 0b1000000, 0b0100000, 0b0011100, 0b0000010, 0b0000001,
                    0b1000001, 0b0111110, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // S
                glyph![
                    0b1111111, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000,
                    0b0001000, 0b0001000, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // T
                glyph![
                    0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b1000001,
                    0b1000001, 0b0111110, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // U
                glyph![
                    0b1000001, 0b1000001, 0b1000001, 0b0100010, 0b0100010, 0b0010100, 0b0010100,
                    0b0001000, 0b0001000, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // V
                glyph![
                    0b1000001, 0b1000001, 0b1000001, 0b1000001, 0b1001001, 0b1001001, 0b0101010,
                    0b0010100, 0b0010100, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // W
                glyph![
                    0b1000001, 0b0100010, 0b0010100, 0b0001000, 0b0001000, 0b0010100, 0b0100010,
                    0b1000001, 0b1000001, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // X
                glyph![
                    0b1000001, 0b0100010, 0b0010100, 0b0001000, 0b0001000, 0b0001000, 0b0001000,
                    0b0001000, 0b0001000, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // Y
                glyph![
                    0b1111111, 0b0000010, 0b0000100, 0b0001000, 0b0010000, 0b0100000, 0b1000000,
                    0b1000000, 0b1111111, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // Z
            ];
            &UPPER[(ch - b'A') as usize]
        }
        b'a'..=b'z' => {
            static LOWER: [[u8; 13]; 26] = [
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b0111100, 0b0000010, 0b0111110, 0b1000010,
                    0b1000010, 0b0111110, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // a
                glyph![
                    0b1000000, 0b1000000, 0b1000000, 0b1011100, 0b1100010, 0b1000010, 0b1000010,
                    0b1100010, 0b1011100, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // b
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b0111100, 0b1000010, 0b1000000, 0b1000000,
                    0b1000010, 0b0111100, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // c
                glyph![
                    0b0000010, 0b0000010, 0b0000010, 0b0111010, 0b1000110, 0b1000010, 0b1000010,
                    0b1000110, 0b0111010, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // d
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b0111100, 0b1000010, 0b1111110, 0b1000000,
                    0b1000010, 0b0111100, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // e
                glyph![
                    0b0001100, 0b0010010, 0b0010000, 0b0010000, 0b1111100, 0b0010000, 0b0010000,
                    0b0010000, 0b0010000, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // f
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b0111010, 0b1000110, 0b1000010, 0b1000110,
                    0b0111010, 0b0000010, 0b1000010, 0b0111100, 0b0000000, 0b0000000
                ], // g
                glyph![
                    0b1000000, 0b1000000, 0b1000000, 0b1011100, 0b1100010, 0b1000010, 0b1000010,
                    0b1000010, 0b1000010, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // h
                glyph![
                    0b0001000, 0b0000000, 0b0000000, 0b0011000, 0b0001000, 0b0001000, 0b0001000,
                    0b0001000, 0b0011100, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // i
                glyph![
                    0b0000100, 0b0000000, 0b0000000, 0b0001100, 0b0000100, 0b0000100, 0b0000100,
                    0b0000100, 0b1000100, 0b1000100, 0b0111000, 0b0000000, 0b0000000
                ], // j
                glyph![
                    0b1000000, 0b1000000, 0b1000000, 0b1000100, 0b1001000, 0b1010000, 0b1110000,
                    0b1001000, 0b1000100, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // k
                glyph![
                    0b0011000, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000, 0b0001000,
                    0b0001000, 0b0011100, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // l
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b1110110, 0b1001001, 0b1001001, 0b1001001,
                    0b1001001, 0b1001001, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // m
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b1011100, 0b1100010, 0b1000010, 0b1000010,
                    0b1000010, 0b1000010, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // n
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b0111100, 0b1000010, 0b1000010, 0b1000010,
                    0b1000010, 0b0111100, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // o
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b1011100, 0b1100010, 0b1000010, 0b1100010,
                    0b1011100, 0b1000000, 0b1000000, 0b1000000, 0b0000000, 0b0000000
                ], // p
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b0111010, 0b1000110, 0b1000010, 0b1000110,
                    0b0111010, 0b0000010, 0b0000010, 0b0000010, 0b0000000, 0b0000000
                ], // q
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b1011100, 0b1100010, 0b1000000, 0b1000000,
                    0b1000000, 0b1000000, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // r
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b0111110, 0b1000000, 0b0111100, 0b0000010,
                    0b0000010, 0b1111100, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // s
                glyph![
                    0b0010000, 0b0010000, 0b0010000, 0b1111100, 0b0010000, 0b0010000, 0b0010000,
                    0b0010010, 0b0001100, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // t
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b1000010, 0b1000010, 0b1000010, 0b1000010,
                    0b1000110, 0b0111010, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // u
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b1000010, 0b1000010, 0b0100100, 0b0100100,
                    0b0011000, 0b0001000, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // v
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b1000001, 0b1001001, 0b1001001, 0b1001001,
                    0b0101010, 0b0010100, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // w
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b1000010, 0b0100100, 0b0011000, 0b0011000,
                    0b0100100, 0b1000010, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // x
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b1000010, 0b1000010, 0b1000110, 0b0111010,
                    0b0000010, 0b1000010, 0b0111100, 0b0000000, 0b0000000, 0b0000000
                ], // y
                glyph![
                    0b0000000, 0b0000000, 0b0000000, 0b1111110, 0b0000100, 0b0001000, 0b0010000,
                    0b0100000, 0b1111110, 0b0000000, 0b0000000, 0b0000000, 0b0000000
                ], // z
            ];
            &LOWER[(ch - b'a') as usize]
        }
        _ => &DEFAULT,
    }
}

fn handle_put_image(state: &ClientState, data: &[u8]) -> Vec<u8> {
    if data.len() < 24 {
        return Vec::new();
    }

    let _format = data[1]; // 0=Bitmap, 1=XYPixmap, 2=ZPixmap
    let drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let _gc = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let width = u16::from_le_bytes([data[12], data[13]]);
    let height = u16::from_le_bytes([data[14], data[15]]);
    let dst_x = i16::from_le_bytes([data[16], data[17]]);
    let dst_y = i16::from_le_bytes([data[18], data[19]]);
    // left_pad at [20], depth at [21]

    let pixel_data = data[24..].to_vec();

    let _ = state.update_tx.send((
        state.client_id.clone(),
        DisplayUpdate::PutImage {
            window_id: drawable,
            x: dst_x,
            y: dst_y,
            width,
            height,
            data: pixel_data,
        },
    ));

    Vec::new()
}

fn handle_get_image(_state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    if data.len() < 20 {
        return Vec::new();
    }

    let _drawable = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let _x = i16::from_le_bytes([data[8], data[9]]);
    let _y = i16::from_le_bytes([data[10], data[11]]);
    let width = u16::from_le_bytes([data[12], data[13]]);
    let height = u16::from_le_bytes([data[14], data[15]]);

    // Return a blank image (all zeros)
    let row_bytes = width as usize * 4; // 32bpp
    let padded_row = (row_bytes + 3) & !3;
    let data_len = padded_row * height as usize;
    let length_field = (data_len / 4) as u32;

    let mut reply = vec![0u8; 32 + data_len];
    reply[0] = 1; // Reply
    reply[1] = 24; // depth
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&length_field.to_le_bytes());
    reply[8..12].copy_from_slice(&ROOT_VISUAL.to_le_bytes());

    reply
}

fn handle_alloc_color(_state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // AllocColor request: [opcode, pad, length, colormap(4), red(2), green(2), blue(2), pad(2)]
    if data.len() < 16 {
        return Vec::new();
    }

    let red = u16::from_le_bytes([data[8], data[9]]);
    let green = u16::from_le_bytes([data[10], data[11]]);
    let blue = u16::from_le_bytes([data[12], data[13]]);

    // For TrueColor visual with masks R=0xFF0000 G=0x00FF00 B=0x0000FF:
    // Convert 16-bit color components to 8-bit and pack into a pixel value
    let r8 = (red >> 8) as u32;
    let g8 = (green >> 8) as u32;
    let b8 = (blue >> 8) as u32;
    let pixel = (r8 << 16) | (g8 << 8) | b8;

    // AllocColor reply: [1, pad, seq(2), length(4)=0, red(2), green(2), blue(2), pad(2), pixel(4)]
    let mut reply = [0u8; 32];
    reply[0] = 1; // Reply
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    // length = 0 (no extra data beyond 32 bytes)
    reply[8..10].copy_from_slice(&red.to_le_bytes());
    reply[10..12].copy_from_slice(&green.to_le_bytes());
    reply[12..14].copy_from_slice(&blue.to_le_bytes());
    // pad at 14..16
    reply[16..20].copy_from_slice(&pixel.to_le_bytes());

    reply.to_vec()
}

fn handle_query_colors(_state: &ClientState, data: &[u8], seq: u16) -> Vec<u8> {
    // QueryColors request: [opcode, pad, length, colormap(4), pixel0(4), pixel1(4), ...]
    if data.len() < 8 {
        return Vec::new();
    }

    let n_pixels = (data.len() - 8) / 4;
    let mut colors = Vec::with_capacity(n_pixels);

    for i in 0..n_pixels {
        let offset = 8 + i * 4;
        let pixel = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);

        // Decompose TrueColor pixel back to 16-bit RGB
        let r = ((pixel >> 16) & 0xFF) as u16;
        let g = ((pixel >> 8) & 0xFF) as u16;
        let b = (pixel & 0xFF) as u16;

        colors.push((r << 8 | r, g << 8 | g, b << 8 | b));
    }

    let data_len = n_pixels * 8; // Each RGB is 8 bytes (r2, g2, b2, pad2)
    let padded = (data_len + 3) & !3;
    let length_field = (padded / 4) as u32;

    let mut reply = vec![0u8; 32 + padded];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    reply[4..8].copy_from_slice(&length_field.to_le_bytes());
    reply[8..10].copy_from_slice(&(n_pixels as u16).to_le_bytes());

    for (i, &(r, g, b)) in colors.iter().enumerate() {
        let off = 32 + i * 8;
        reply[off..off + 2].copy_from_slice(&r.to_le_bytes());
        reply[off + 2..off + 4].copy_from_slice(&g.to_le_bytes());
        reply[off + 4..off + 6].copy_from_slice(&b.to_le_bytes());
        // pad at off+6..off+8
    }

    reply
}

fn handle_query_extension(_state: &ClientState, _data: &[u8], seq: u16) -> Vec<u8> {
    // Reply "extension not found" for all extensions
    let mut reply = [0u8; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&seq.to_le_bytes());
    // present = false (byte 8 = 0) — already zero
    reply.to_vec()
}
