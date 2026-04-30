//! Translation between the backend's internal protocol-crate enums
//! and the Cap'n Proto wire types from the `wire` crate.
//!
//! Mirror of `crates/sidecar-macos/src/wire_bridge.rs`. Each side
//! owns its own bridge so the wire crate stays free of the
//! `protocol` crate; the symmetry costs duplication but keeps the
//! transport layer reusable across sidecar implementations.
//!
//! Direction:
//!   - `read_from_sidecar(...)` parses an inbound `FromSidecar`
//!     into the matching `SidecarToBackend` variant.
//!   - `build_to_sidecar(...)` serializes an outbound
//!     `BackendToSidecar` into a `ToSidecar` builder.

use capnp::message::{Builder, HeapAllocator};
use tracing::warn;
use x11_web_protocol::{
    AnimCursorFrame, BackendToSidecar, DisplayUpdate, DndEventKind, GesturePhase, InputEvent,
    MenuAction, MenuItem, MenuItemKind, ProcessInfo, SidecarToBackend, WindowWmState,
};
use x11_web_wire::wire_capnp;

#[derive(Debug)]
#[allow(dead_code)]
pub enum BridgeError {
    /// Reserved for future variants the backend doesn't yet
    /// recognize. Currently every variant in `from_sidecar` is
    /// translated, but keeping this around lets us return a
    /// distinct error type on schema additions without an API
    /// break.
    Unsupported(&'static str),
    Capnp(capnp::Error),
    Utf8(std::str::Utf8Error),
}

impl From<capnp::Error> for BridgeError {
    fn from(e: capnp::Error) -> Self {
        BridgeError::Capnp(e)
    }
}

impl From<std::str::Utf8Error> for BridgeError {
    fn from(e: std::str::Utf8Error) -> Self {
        BridgeError::Utf8(e)
    }
}

impl From<capnp::NotInSchema> for BridgeError {
    fn from(e: capnp::NotInSchema) -> Self {
        BridgeError::Capnp(capnp::Error::failed(format!("not in schema: {e:?}")))
    }
}

// ---------------------------------------------------------------------------
// Inbound: wire_capnp::FromSidecar → SidecarToBackend
// ---------------------------------------------------------------------------

pub fn read_from_sidecar(
    reader: wire_capnp::from_sidecar::Reader,
) -> Result<SidecarToBackend, BridgeError> {
    use wire_capnp::from_sidecar::Which;
    Ok(match reader.which()? {
        Which::Heartbeat(()) => SidecarToBackend::Heartbeat,
        Which::ProcessConnected(pc) => {
            let pc = pc?;
            SidecarToBackend::ProcessConnected {
                pid: pc.get_pid(),
                client_id: pc.get_client_id()?.to_string()?,
                command: pc.get_command()?.to_string()?,
            }
        }
        Which::ProcessExited(pe) => {
            let pe = pe?;
            SidecarToBackend::ProcessExited {
                pid: pe.get_pid(),
                exit_code: if pe.get_has_exit_code() {
                    Some(pe.get_exit_code())
                } else {
                    None
                },
            }
        }
        Which::Display(du) => {
            let du = du?;
            let client_id = du.get_client_id()?.to_string()?;
            let update = read_display_payload(du.get_payload()?)?;
            SidecarToBackend::DisplayUpdate { client_id, update }
        }
        Which::InputDropped(idr) => {
            let idr = idr?;
            SidecarToBackend::InputDropped {
                window_id: idr.get_window_id()?.to_string()?,
                reason: idr.get_reason()?.to_string()?,
            }
        }
        Which::ProcessSpawned(ps) => {
            let ps = ps?;
            SidecarToBackend::ProcessSpawned {
                request_id: ps.get_request_id()?.to_string()?,
                pid: ps.get_pid(),
            }
        }
        Which::ProcessKilled(pk) => {
            let pk = pk?;
            SidecarToBackend::ProcessKilled {
                request_id: pk.get_request_id()?.to_string()?,
                pid: pk.get_pid(),
            }
        }
        Which::ProcessList(pl) => {
            let pl = pl?;
            let processes = pl.get_processes()?;
            let mut out = Vec::with_capacity(processes.len() as usize);
            for entry in processes.iter() {
                out.push(ProcessInfo {
                    pid: entry.get_pid(),
                    command: entry.get_command()?.to_string()?,
                });
            }
            SidecarToBackend::ProcessList {
                request_id: pl.get_request_id()?.to_string()?,
                processes: out,
            }
        }
        Which::ErrorReply(er) => {
            let er = er?;
            SidecarToBackend::Error {
                request_id: if er.get_has_request_id() {
                    Some(er.get_request_id()?.to_string()?)
                } else {
                    None
                },
                message: er.get_message()?.to_string()?,
            }
        }
        Which::ClipboardOffer(co) => {
            let co = co?;
            let mime_types = co.get_mime_types()?;
            let mut mts = Vec::with_capacity(mime_types.len() as usize);
            for entry in mime_types.iter() {
                mts.push(entry?.to_string()?);
            }
            SidecarToBackend::ClipboardOffer {
                selection: co.get_selection()?.to_string()?,
                mime_types: mts,
            }
        }
        Which::ClipboardData(cd) => {
            let cd = cd?;
            SidecarToBackend::ClipboardData {
                selection: cd.get_selection()?.to_string()?,
                mime_type: cd.get_mime_type()?.to_string()?,
                data: cd.get_data()?.to_vec(),
            }
        }
    })
}

fn read_display_payload(
    payload: wire_capnp::display_payload::Reader,
) -> Result<DisplayUpdate, BridgeError> {
    use wire_capnp::display_payload::Which;
    Ok(match payload.which()? {
        Which::WindowCreated(wc) => {
            let wc = wc?;
            DisplayUpdate::WindowCreated {
                window_id: wc.get_window_id()?.to_string()?,
                x: wc.get_x(),
                y: wc.get_y(),
                width: wc.get_width(),
                height: wc.get_height(),
                is_top_level: wc.get_is_top_level(),
                override_redirect: wc.get_override_redirect(),
                border_width: wc.get_border_width(),
                border_pixel: wc.get_border_pixel(),
            }
        }
        Which::WindowDestroyed(wd) => {
            let wd = wd?;
            DisplayUpdate::WindowDestroyed {
                window_id: wd.get_window_id()?.to_string()?,
            }
        }
        Which::WindowMapped(wm) => {
            let wm = wm?;
            DisplayUpdate::WindowMapped {
                window_id: wm.get_window_id()?.to_string()?,
                is_top_level: wm.get_is_top_level(),
                override_redirect: wm.get_override_redirect(),
            }
        }
        Which::WindowUnmapped(wu) => {
            let wu = wu?;
            DisplayUpdate::WindowUnmapped {
                window_id: wu.get_window_id()?.to_string()?,
            }
        }
        Which::WindowConfigured(wc) => {
            let wc = wc?;
            DisplayUpdate::WindowConfigured {
                window_id: wc.get_window_id()?.to_string()?,
                x: wc.get_x(),
                y: wc.get_y(),
                width: wc.get_width(),
                height: wc.get_height(),
                border_width: wc.get_border_width(),
                border_pixel: wc.get_border_pixel(),
            }
        }
        Which::TitleChanged(tc) => {
            let tc = tc?;
            DisplayUpdate::TitleChanged {
                window_id: tc.get_window_id()?.to_string()?,
                title: tc.get_title()?.to_string()?,
            }
        }
        Which::PutImage(pi) => {
            let pi = pi?;
            DisplayUpdate::PutImage {
                window_id: pi.get_window_id()?.to_string()?,
                x: pi.get_x(),
                y: pi.get_y(),
                width: pi.get_width(),
                height: pi.get_height(),
                data: pi.get_data()?.to_vec(),
            }
        }
        Which::CursorChanged(cc) => {
            let cc = cc?;
            DisplayUpdate::CursorChanged {
                window_id: cc.get_window_id()?.to_string()?,
                cursor: cc.get_cursor()?.to_string()?,
            }
        }
        Which::CursorBitmap(cb) => {
            let cb = cb?;
            DisplayUpdate::CursorBitmap {
                window_id: cb.get_window_id()?.to_string()?,
                width: cb.get_width(),
                height: cb.get_height(),
                hotspot_x: cb.get_hotspot_x(),
                hotspot_y: cb.get_hotspot_y(),
                data: cb.get_data()?.to_vec(),
            }
        }
        Which::CursorAnimated(ca) => {
            let ca = ca?;
            let frames = ca.get_frames()?;
            let mut out = Vec::with_capacity(frames.len() as usize);
            for frame in frames.iter() {
                out.push(AnimCursorFrame {
                    pixels: frame.get_pixels()?.to_vec(),
                    width: frame.get_width(),
                    height: frame.get_height(),
                    hotspot_x: frame.get_hotspot_x(),
                    hotspot_y: frame.get_hotspot_y(),
                    delay_ms: frame.get_delay_ms(),
                });
            }
            DisplayUpdate::CursorAnimated {
                window_id: ca.get_window_id()?.to_string()?,
                frames: out,
            }
        }
        Which::WindowFocused(wf) => {
            let wf = wf?;
            DisplayUpdate::WindowFocused {
                window_id: if wf.get_has_window_id() {
                    Some(wf.get_window_id()?.to_string()?)
                } else {
                    None
                },
            }
        }
        Which::WindowRaised(wr) => {
            let wr = wr?;
            DisplayUpdate::WindowRaised {
                window_id: wr.get_window_id()?.to_string()?,
            }
        }
        Which::WindowStateChanged(ws) => {
            let ws = ws?;
            DisplayUpdate::WindowStateChanged {
                window_id: ws.get_window_id()?.to_string()?,
                state: read_wm_state(ws.get_state()?),
            }
        }
        Which::WindowUrgent(wu) => {
            let wu = wu?;
            DisplayUpdate::WindowUrgent {
                window_id: wu.get_window_id()?.to_string()?,
                urgent: wu.get_urgent(),
            }
        }
        Which::TransientForSet(tfs) => {
            let tfs = tfs?;
            DisplayUpdate::TransientForSet {
                window_id: tfs.get_window_id()?.to_string()?,
                parent_window_id: if tfs.get_has_parent() {
                    Some(tfs.get_parent_window_id()?.to_string()?)
                } else {
                    None
                },
            }
        }
        Which::WindowIconChanged(wic) => {
            let wic = wic?;
            DisplayUpdate::WindowIconChanged {
                window_id: wic.get_window_id()?.to_string()?,
                width: wic.get_width(),
                height: wic.get_height(),
                data: wic.get_data()?.to_vec(),
            }
        }
        Which::Bell(b) => {
            let b = b?;
            DisplayUpdate::Bell {
                percent: b.get_percent(),
            }
        }
        Which::MenuStructure(ms) => {
            let ms = ms?;
            let menu = read_menu_items(ms.get_menu()?)?;
            DisplayUpdate::MenuStructure {
                window_id: ms.get_window_id()?.to_string()?,
                menu,
            }
        }
        Which::MenuStateChanged(ms) => {
            let ms = ms?;
            DisplayUpdate::MenuStateChanged {
                window_id: ms.get_window_id()?.to_string()?,
                item_id: ms.get_item_id()?.to_string()?,
                enabled: if ms.get_has_enabled() {
                    Some(ms.get_enabled())
                } else {
                    None
                },
                checked: if ms.get_has_checked() {
                    Some(ms.get_checked())
                } else {
                    None
                },
                label: if ms.get_has_label() {
                    Some(ms.get_label()?.to_string()?)
                } else {
                    None
                },
            }
        }
    })
}

fn read_wm_state(state: wire_capnp::WindowWmState) -> WindowWmState {
    use wire_capnp::WindowWmState as W;
    match state {
        W::Normal => WindowWmState::Normal,
        W::Minimized => WindowWmState::Minimized,
        W::Maximized => WindowWmState::Maximized,
        W::Fullscreen => WindowWmState::Fullscreen,
        W::Close => WindowWmState::Close,
    }
}

fn read_menu_items(
    list: capnp::struct_list::Reader<wire_capnp::menu_item::Owned>,
) -> Result<Vec<MenuItem>, BridgeError> {
    let mut out = Vec::with_capacity(list.len() as usize);
    for entry in list.iter() {
        out.push(read_menu_item(entry)?);
    }
    Ok(out)
}

fn read_menu_item(item: wire_capnp::menu_item::Reader) -> Result<MenuItem, BridgeError> {
    let kind = match item.get_kind()? {
        wire_capnp::MenuItemKind::Normal => MenuItemKind::Normal,
        wire_capnp::MenuItemKind::Submenu => MenuItemKind::Submenu,
        wire_capnp::MenuItemKind::Separator => MenuItemKind::Separator,
        wire_capnp::MenuItemKind::Checkbox => MenuItemKind::Checkbox,
        wire_capnp::MenuItemKind::Radio => MenuItemKind::Radio,
    };
    let action = if item.get_has_action() {
        let a = item.get_action()?;
        Some(read_menu_action(a)?)
    } else {
        None
    };
    Ok(MenuItem {
        id: item.get_id()?.to_string()?,
        label: if item.get_has_label() {
            Some(item.get_label()?.to_string()?)
        } else {
            None
        },
        kind,
        enabled: item.get_enabled(),
        visible: item.get_visible(),
        checked: if item.get_has_checked() {
            Some(item.get_checked())
        } else {
            None
        },
        accelerator: if item.get_has_accelerator() {
            Some(item.get_accelerator()?.to_string()?)
        } else {
            None
        },
        icon: if item.get_has_icon() {
            Some(item.get_icon()?.to_string()?)
        } else {
            None
        },
        action,
        children: read_menu_items(item.get_children()?)?,
    })
}

fn read_menu_action(a: wire_capnp::menu_action::Reader) -> Result<MenuAction, BridgeError> {
    let target = if a.get_has_target() {
        let txt = a.get_target_json()?.to_string()?;
        // The wire carries `target` as JSON text since Cap'n Proto
        // has no native dynamic-value type. The protocol crate
        // expects `Option<serde_json::Value>` — round-trip via
        // serde_json. Bad JSON is logged + treated as `None`.
        match serde_json::from_str::<serde_json::Value>(&txt) {
            Ok(v) => Some(v),
            Err(e) => {
                warn!("MenuAction.target invalid JSON: {e}");
                None
            }
        }
    } else {
        None
    };
    Ok(MenuAction {
        name: a.get_name()?.to_string()?,
        target,
    })
}

// ---------------------------------------------------------------------------
// Outbound: BackendToSidecar → wire_capnp::ToSidecar
// ---------------------------------------------------------------------------

pub fn build_to_sidecar(msg: &BackendToSidecar) -> Option<Builder<HeapAllocator>> {
    let mut builder = Builder::new_default();
    {
        let root = builder.init_root::<wire_capnp::to_sidecar::Builder>();
        match msg {
            BackendToSidecar::InputEvent { window_id, event } => {
                let mut env = root.init_input_event();
                env.set_window_id(window_id);
                let event_b = env.init_event();
                if !write_input_event(event_b, event) {
                    return None;
                }
            }
            BackendToSidecar::RequestRedraw { window_id } => {
                let mut rr = root.init_request_redraw();
                rr.set_window_id(window_id);
            }
            BackendToSidecar::ResizeWindow {
                window_id,
                width,
                height,
            } => {
                let mut rw = root.init_resize_window();
                rw.set_window_id(window_id);
                rw.set_width(*width);
                rw.set_height(*height);
            }
            BackendToSidecar::SpawnProcess {
                request_id,
                command,
                args,
            } => {
                let mut sp = root.init_spawn_process();
                sp.set_request_id(request_id);
                sp.set_command(command);
                let mut list = sp.init_args(args.len() as u32);
                for (i, arg) in args.iter().enumerate() {
                    list.set(i as u32, arg);
                }
            }
            BackendToSidecar::KillProcess { request_id, pid } => {
                let mut kp = root.init_kill_process();
                kp.set_request_id(request_id);
                kp.set_pid(*pid);
            }
            BackendToSidecar::ListProcesses { request_id } => {
                let mut lp = root.init_list_processes();
                lp.set_request_id(request_id);
            }
            BackendToSidecar::RequestClipboard {
                selection,
                mime_type,
            } => {
                let mut rc = root.init_request_clipboard();
                rc.set_selection(selection);
                rc.set_mime_type(mime_type);
            }
            BackendToSidecar::SetClipboard {
                selection,
                mime_type,
                data,
            } => {
                let mut sc = root.init_set_clipboard();
                sc.set_selection(selection);
                sc.set_mime_type(mime_type);
                sc.set_data(data);
            }
            BackendToSidecar::ResizeScreen { width, height } => {
                let mut rs = root.init_resize_screen();
                rs.set_width(*width);
                rs.set_height(*height);
            }
            // RTC variants are deliberately not part of the new
            // sidecar↔backend protocol — the architectural plan
            // moves WebRTC entirely to the frontend↔backend hop.
            BackendToSidecar::RtcAnswer { .. } | BackendToSidecar::RtcIceCandidate { .. } => {
                return None;
            }
        }
    }
    Some(builder)
}

fn write_input_event(builder: wire_capnp::input_event::Builder, event: &InputEvent) -> bool {
    match event {
        InputEvent::KeyPress { keycode, state } => {
            let mut kp = builder.init_key_press();
            kp.set_keycode(*keycode);
            kp.set_state(*state);
        }
        InputEvent::KeyRelease { keycode, state } => {
            let mut kr = builder.init_key_release();
            kr.set_keycode(*keycode);
            kr.set_state(*state);
        }
        InputEvent::ButtonPress {
            button,
            x,
            y,
            state,
        } => {
            let mut bp = builder.init_button_press();
            bp.set_button(*button);
            bp.set_x(*x);
            bp.set_y(*y);
            bp.set_state(*state);
        }
        InputEvent::ButtonRelease {
            button,
            x,
            y,
            state,
        } => {
            let mut br = builder.init_button_release();
            br.set_button(*button);
            br.set_x(*x);
            br.set_y(*y);
            br.set_state(*state);
        }
        InputEvent::MotionNotify { x, y, state } => {
            let mut mn = builder.init_motion_notify();
            mn.set_x(*x);
            mn.set_y(*y);
            mn.set_state(*state);
        }
        InputEvent::MenuActivate { action } => {
            let ma = builder.init_menu_activate();
            write_menu_action(ma.init_action(), action);
        }
        InputEvent::WindowManage { action } => {
            let mut wm = builder.init_window_manage();
            wm.set_action(write_wm_state(*action));
        }
        InputEvent::DndBridge { event } => {
            let db = builder.init_dnd_bridge();
            write_dnd_event(db.init_event(), event);
        }
        InputEvent::TouchBegin {
            touch_id,
            x,
            y,
            state,
        } => {
            let mut t = builder.init_touch_begin();
            t.set_touch_id(*touch_id);
            t.set_x(*x);
            t.set_y(*y);
            t.set_state(*state);
        }
        InputEvent::TouchUpdate {
            touch_id,
            x,
            y,
            state,
        } => {
            let mut t = builder.init_touch_update();
            t.set_touch_id(*touch_id);
            t.set_x(*x);
            t.set_y(*y);
            t.set_state(*state);
        }
        InputEvent::TouchEnd {
            touch_id,
            x,
            y,
            state,
        } => {
            let mut t = builder.init_touch_end();
            t.set_touch_id(*touch_id);
            t.set_x(*x);
            t.set_y(*y);
            t.set_state(*state);
        }
        InputEvent::GestureSwipe {
            phase,
            fingers,
            dx,
            dy,
        } => {
            let mut gs = builder.init_gesture_swipe();
            gs.set_phase(write_gesture_phase(phase));
            gs.set_fingers(*fingers);
            gs.set_dx(*dx);
            gs.set_dy(*dy);
        }
        InputEvent::GesturePinch {
            phase,
            fingers,
            dx,
            dy,
            scale,
            rotation,
        } => {
            let mut gp = builder.init_gesture_pinch();
            gp.set_phase(write_gesture_phase(phase));
            gp.set_fingers(*fingers);
            gp.set_dx(*dx);
            gp.set_dy(*dy);
            gp.set_scale(*scale);
            gp.set_rotation(*rotation);
        }
        InputEvent::CompositionEvent { phase, text } => {
            let mut ce = builder.init_composition_event();
            ce.set_phase(phase);
            ce.set_text(text);
        }
    }
    true
}

fn write_wm_state(state: WindowWmState) -> wire_capnp::WindowWmState {
    use wire_capnp::WindowWmState as W;
    match state {
        WindowWmState::Normal => W::Normal,
        WindowWmState::Minimized => W::Minimized,
        WindowWmState::Maximized => W::Maximized,
        WindowWmState::Fullscreen => W::Fullscreen,
        WindowWmState::Close => W::Close,
    }
}

fn write_gesture_phase(phase: &GesturePhase) -> wire_capnp::GesturePhase {
    use wire_capnp::GesturePhase as G;
    match phase {
        GesturePhase::Begin => G::Begin,
        GesturePhase::Update => G::Update,
        GesturePhase::End => G::End,
    }
}

fn write_menu_action(mut b: wire_capnp::menu_action::Builder, action: &MenuAction) {
    b.set_name(&action.name);
    if let Some(target) = &action.target {
        b.set_has_target(true);
        // JSON-serialize target back onto the wire. The protocol
        // crate's `target` is a `serde_json::Value`; we already
        // round-trip via JSON text on the read side.
        let s = serde_json::to_string(target).unwrap_or_else(|_| "null".into());
        b.set_target_json(&s);
    } else {
        b.set_has_target(false);
    }
}

fn write_dnd_event(mut builder: wire_capnp::dnd_event_kind::Builder, event: &DndEventKind) {
    match event {
        DndEventKind::Enter { mime_types } => {
            let enter = builder.init_enter();
            let mut list = enter.init_mime_types(mime_types.len() as u32);
            for (i, mt) in mime_types.iter().enumerate() {
                list.set(i as u32, mt);
            }
        }
        DndEventKind::Position { x, y } => {
            let mut p = builder.init_position();
            p.set_x(*x);
            p.set_y(*y);
        }
        DndEventKind::Drop { mime_type, data } => {
            let mut d = builder.init_drop();
            d.set_mime_type(mime_type);
            d.set_data(data);
        }
        DndEventKind::Leave => {
            builder.set_leave(());
        }
    }
}
