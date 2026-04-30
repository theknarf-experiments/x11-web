//! Translation between the high-level `BackendToSidecar` /
//! `SidecarToBackend` Rust enums and the Cap'n Proto wire types in
//! `wire_capnp`.
//!
//! Single home for the four conversion entry points:
//!   - `build_from_sidecar`  (sidecar → backend, write)
//!   - `read_from_sidecar`   (sidecar → backend, read)
//!   - `build_to_sidecar`    (backend → sidecar, write)
//!   - `read_to_sidecar`     (backend → sidecar, read)
//!
//! Each side of the wire only needs two of these, but every variant
//! is exhaustively handled so different sidecar implementations
//! (X11, macOS) can share the same translation tables. A sidecar
//! that never emits, say, `MenuStructure` simply doesn't construct
//! that variant — there's no per-implementation pruning here.

use capnp::message::{Builder, HeapAllocator};
use x11_web_protocol::{
    AnimCursorFrame, DisplayUpdate, DndEventKind, GesturePhase, InputEvent, MenuAction,
    MenuActionTarget, MenuItem, MenuItemKind, ProcessInfo, WindowWmState,
};

use crate::types::{BackendToSidecar, SidecarToBackend};
use crate::wire_capnp;

#[derive(Debug)]
pub enum BridgeError {
    /// A wire-side variant the bridge doesn't translate. Reserved
    /// for forward-compat: schema additions can return this without
    /// changing the API.
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
// Outbound: SidecarToBackend → wire_capnp::FromSidecar
// ---------------------------------------------------------------------------

pub fn build_from_sidecar(msg: &SidecarToBackend) -> Option<Builder<HeapAllocator>> {
    let mut builder = Builder::new_default();
    {
        let mut root = builder.init_root::<wire_capnp::from_sidecar::Builder>();
        match msg {
            SidecarToBackend::Heartbeat => {
                root.set_heartbeat(());
            }
            SidecarToBackend::ProcessConnected {
                pid,
                client_id,
                command,
            } => {
                let mut pc = root.init_process_connected();
                pc.set_pid(*pid);
                pc.set_client_id(client_id);
                pc.set_command(command);
            }
            SidecarToBackend::ProcessExited { pid, exit_code } => {
                let mut pe = root.init_process_exited();
                pe.set_pid(*pid);
                let mut es = pe.init_exit_status();
                match exit_code {
                    Some(code) => es.set_code(*code),
                    None => es.set_killed_by_signal(()),
                }
            }
            SidecarToBackend::ProcessSpawned { request_id, pid } => {
                let mut ps = root.init_process_spawned();
                ps.set_request_id(request_id);
                ps.set_pid(*pid);
            }
            SidecarToBackend::ProcessKilled { request_id, pid } => {
                let mut pk = root.init_process_killed();
                pk.set_request_id(request_id);
                pk.set_pid(*pid);
            }
            SidecarToBackend::ProcessList {
                request_id,
                processes,
            } => {
                let mut pl = root.init_process_list();
                pl.set_request_id(request_id);
                let mut list = pl.init_processes(processes.len() as u32);
                for (i, p) in processes.iter().enumerate() {
                    let mut entry = list.reborrow().get(i as u32);
                    entry.set_pid(p.pid);
                    entry.set_command(&p.command);
                }
            }
            SidecarToBackend::Error {
                request_id,
                message,
            } => {
                let mut er = root.init_error_reply();
                if let Some(id) = request_id {
                    er.set_request_id(id);
                }
                er.set_message(message);
            }
            SidecarToBackend::DisplayUpdate { client_id, update } => {
                let mut display = root.init_display();
                display.set_client_id(client_id);
                let payload = display.init_payload();
                if !write_display_payload(payload, update) {
                    return None;
                }
            }
            SidecarToBackend::InputDropped { window_id, reason } => {
                let mut idr = root.init_input_dropped();
                idr.set_window_id(window_id);
                idr.set_reason(reason);
            }
            SidecarToBackend::ClipboardOffer {
                selection,
                mime_types,
            } => {
                let mut co = root.init_clipboard_offer();
                co.set_selection(selection);
                let mut list = co.init_mime_types(mime_types.len() as u32);
                for (i, mt) in mime_types.iter().enumerate() {
                    list.set(i as u32, mt);
                }
            }
            SidecarToBackend::ClipboardData {
                selection,
                mime_type,
                data,
            } => {
                let mut cd = root.init_clipboard_data();
                cd.set_selection(selection);
                cd.set_mime_type(mime_type);
                cd.set_data(data);
            }
        }
    }
    Some(builder)
}

fn write_display_payload(
    payload: wire_capnp::display_payload::Builder,
    update: &DisplayUpdate,
) -> bool {
    match update {
        DisplayUpdate::WindowCreated {
            window_id,
            x,
            y,
            width,
            height,
            is_top_level,
            override_redirect,
            border_width,
            border_pixel,
        } => {
            let mut wc = payload.init_window_created();
            wc.set_window_id(window_id);
            wc.set_x(*x);
            wc.set_y(*y);
            wc.set_width(*width);
            wc.set_height(*height);
            wc.set_is_top_level(*is_top_level);
            wc.set_override_redirect(*override_redirect);
            wc.set_border_width(*border_width);
            wc.set_border_pixel(*border_pixel);
        }
        DisplayUpdate::WindowDestroyed { window_id } => {
            let mut wd = payload.init_window_destroyed();
            wd.set_window_id(window_id);
        }
        DisplayUpdate::WindowMapped {
            window_id,
            is_top_level,
            override_redirect,
        } => {
            let mut wm = payload.init_window_mapped();
            wm.set_window_id(window_id);
            wm.set_is_top_level(*is_top_level);
            wm.set_override_redirect(*override_redirect);
        }
        DisplayUpdate::WindowUnmapped { window_id } => {
            let mut wu = payload.init_window_unmapped();
            wu.set_window_id(window_id);
        }
        DisplayUpdate::WindowConfigured {
            window_id,
            x,
            y,
            width,
            height,
            border_width,
            border_pixel,
        } => {
            let mut wc = payload.init_window_configured();
            wc.set_window_id(window_id);
            wc.set_x(*x);
            wc.set_y(*y);
            wc.set_width(*width);
            wc.set_height(*height);
            wc.set_border_width(*border_width);
            wc.set_border_pixel(*border_pixel);
        }
        DisplayUpdate::TitleChanged { window_id, title } => {
            let mut tc = payload.init_title_changed();
            tc.set_window_id(window_id);
            tc.set_title(title);
        }
        DisplayUpdate::PutImage {
            window_id,
            x,
            y,
            width,
            height,
            data,
        } => {
            let mut pi = payload.init_put_image();
            pi.set_window_id(window_id);
            pi.set_x(*x);
            pi.set_y(*y);
            pi.set_width(*width);
            pi.set_height(*height);
            pi.set_encoding(wire_capnp::ImageEncoding::RawRgba);
            pi.set_data(data);
        }
        DisplayUpdate::CursorChanged { window_id, cursor } => {
            let mut cc = payload.init_cursor_changed();
            cc.set_window_id(window_id);
            cc.set_cursor(cursor);
        }
        DisplayUpdate::CursorBitmap {
            window_id,
            width,
            height,
            hotspot_x,
            hotspot_y,
            data,
        } => {
            let mut cb = payload.init_cursor_bitmap();
            cb.set_window_id(window_id);
            cb.set_width(*width);
            cb.set_height(*height);
            cb.set_hotspot_x(*hotspot_x);
            cb.set_hotspot_y(*hotspot_y);
            cb.set_data(data);
        }
        DisplayUpdate::CursorAnimated { window_id, frames } => {
            let mut ca = payload.init_cursor_animated();
            ca.set_window_id(window_id);
            let mut fl = ca.init_frames(frames.len() as u32);
            for (i, f) in frames.iter().enumerate() {
                let mut entry = fl.reborrow().get(i as u32);
                entry.set_pixels(&f.pixels);
                entry.set_width(f.width);
                entry.set_height(f.height);
                entry.set_hotspot_x(f.hotspot_x);
                entry.set_hotspot_y(f.hotspot_y);
                entry.set_delay_ms(f.delay_ms);
            }
        }
        DisplayUpdate::WindowFocused { window_id } => {
            let mut wf = payload.init_window_focused();
            if let Some(id) = window_id {
                wf.set_window_id(id);
            }
        }
        DisplayUpdate::WindowRaised { window_id } => {
            let mut wr = payload.init_window_raised();
            wr.set_window_id(window_id);
        }
        DisplayUpdate::WindowStateChanged { window_id, state } => {
            let mut ws = payload.init_window_state_changed();
            ws.set_window_id(window_id);
            ws.set_state(write_wm_state(*state));
        }
        DisplayUpdate::Bell { percent } => {
            let mut b = payload.init_bell();
            b.set_percent(*percent);
        }
        DisplayUpdate::MenuStructure { window_id, menu } => {
            let mut ms = payload.init_menu_structure();
            ms.set_window_id(window_id);
            let list = ms.init_menu(menu.len() as u32);
            write_menu_items(list, menu);
        }
    }
    true
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
            use wire_capnp::process_exited::exit_status::Which as ExitWhich;
            let exit_code = match pe.get_exit_status().which()? {
                ExitWhich::Code(c) => Some(c),
                ExitWhich::KilledBySignal(()) => None,
            };
            SidecarToBackend::ProcessExited {
                pid: pe.get_pid(),
                exit_code,
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
                request_id: if er.has_request_id() {
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
                window_id: if wf.has_window_id() {
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

// ---------------------------------------------------------------------------
// Inbound: wire_capnp::ToSidecar → BackendToSidecar
// ---------------------------------------------------------------------------

pub fn read_to_sidecar(
    reader: wire_capnp::to_sidecar::Reader,
) -> Result<BackendToSidecar, BridgeError> {
    use wire_capnp::to_sidecar::Which;
    Ok(match reader.which()? {
        Which::InputEvent(env) => {
            let env = env?;
            let window_id = env.get_window_id()?.to_string()?;
            let event = read_input_event(env.get_event()?)?;
            BackendToSidecar::InputEvent { window_id, event }
        }
        Which::RequestRedraw(rr) => {
            let rr = rr?;
            BackendToSidecar::RequestRedraw {
                window_id: rr.get_window_id()?.to_string()?,
            }
        }
        Which::ResizeWindow(rw) => {
            let rw = rw?;
            BackendToSidecar::ResizeWindow {
                window_id: rw.get_window_id()?.to_string()?,
                width: rw.get_width(),
                height: rw.get_height(),
            }
        }
        Which::SpawnProcess(sp) => {
            let sp = sp?;
            let args_list = sp.get_args()?;
            let mut args = Vec::with_capacity(args_list.len() as usize);
            for entry in args_list.iter() {
                args.push(entry?.to_string()?);
            }
            BackendToSidecar::SpawnProcess {
                request_id: sp.get_request_id()?.to_string()?,
                command: sp.get_command()?.to_string()?,
                args,
            }
        }
        Which::KillProcess(kp) => {
            let kp = kp?;
            BackendToSidecar::KillProcess {
                request_id: kp.get_request_id()?.to_string()?,
                pid: kp.get_pid(),
            }
        }
        Which::ListProcesses(lp) => {
            let lp = lp?;
            BackendToSidecar::ListProcesses {
                request_id: lp.get_request_id()?.to_string()?,
            }
        }
        Which::RequestClipboard(rc) => {
            let rc = rc?;
            BackendToSidecar::RequestClipboard {
                selection: rc.get_selection()?.to_string()?,
                mime_type: rc.get_mime_type()?.to_string()?,
            }
        }
        Which::SetClipboard(sc) => {
            let sc = sc?;
            BackendToSidecar::SetClipboard {
                selection: sc.get_selection()?.to_string()?,
                mime_type: sc.get_mime_type()?.to_string()?,
                data: sc.get_data()?.to_vec(),
            }
        }
        Which::ResizeScreen(rs) => {
            let rs = rs?;
            BackendToSidecar::ResizeScreen {
                width: rs.get_width(),
                height: rs.get_height(),
            }
        }
    })
}

fn read_input_event(reader: wire_capnp::input_event::Reader) -> Result<InputEvent, BridgeError> {
    use wire_capnp::input_event::Which;
    Ok(match reader.which()? {
        Which::KeyPress(kp) => {
            let kp = kp?;
            InputEvent::KeyPress {
                keycode: kp.get_keycode(),
                state: kp.get_state(),
            }
        }
        Which::KeyRelease(kr) => {
            let kr = kr?;
            InputEvent::KeyRelease {
                keycode: kr.get_keycode(),
                state: kr.get_state(),
            }
        }
        Which::ButtonPress(bp) => {
            let bp = bp?;
            InputEvent::ButtonPress {
                button: bp.get_button(),
                x: bp.get_x(),
                y: bp.get_y(),
                state: bp.get_state(),
            }
        }
        Which::ButtonRelease(br) => {
            let br = br?;
            InputEvent::ButtonRelease {
                button: br.get_button(),
                x: br.get_x(),
                y: br.get_y(),
                state: br.get_state(),
            }
        }
        Which::MotionNotify(mn) => {
            let mn = mn?;
            InputEvent::MotionNotify {
                x: mn.get_x(),
                y: mn.get_y(),
                state: mn.get_state(),
            }
        }
        Which::MenuActivate(ma) => {
            let ma = ma?;
            InputEvent::MenuActivate {
                action: read_menu_action(ma.get_action()?)?,
            }
        }
        Which::WindowManage(wm) => {
            let wm = wm?;
            InputEvent::WindowManage {
                action: read_wm_state(wm.get_action()?),
            }
        }
        Which::DndBridge(db) => {
            let db = db?;
            InputEvent::DndBridge {
                event: read_dnd_event(db.get_event()?)?,
            }
        }
        Which::TouchBegin(t) => {
            let t = t?;
            InputEvent::TouchBegin {
                touch_id: t.get_touch_id(),
                x: t.get_x(),
                y: t.get_y(),
                state: t.get_state(),
            }
        }
        Which::TouchUpdate(t) => {
            let t = t?;
            InputEvent::TouchUpdate {
                touch_id: t.get_touch_id(),
                x: t.get_x(),
                y: t.get_y(),
                state: t.get_state(),
            }
        }
        Which::TouchEnd(t) => {
            let t = t?;
            InputEvent::TouchEnd {
                touch_id: t.get_touch_id(),
                x: t.get_x(),
                y: t.get_y(),
                state: t.get_state(),
            }
        }
        Which::GestureSwipe(g) => {
            let g = g?;
            InputEvent::GestureSwipe {
                phase: read_gesture_phase(g.get_phase()?),
                fingers: g.get_fingers(),
                dx: g.get_dx(),
                dy: g.get_dy(),
            }
        }
        Which::GesturePinch(g) => {
            let g = g?;
            InputEvent::GesturePinch {
                phase: read_gesture_phase(g.get_phase()?),
                fingers: g.get_fingers(),
                dx: g.get_dx(),
                dy: g.get_dy(),
                scale: g.get_scale(),
                rotation: g.get_rotation(),
            }
        }
        Which::CompositionEvent(ce) => {
            let ce = ce?;
            InputEvent::CompositionEvent {
                phase: ce.get_phase()?.to_string()?,
                text: ce.get_text()?.to_string()?,
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Shared helpers (read + write paired)
// ---------------------------------------------------------------------------

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

fn write_gesture_phase(phase: &GesturePhase) -> wire_capnp::GesturePhase {
    use wire_capnp::GesturePhase as G;
    match phase {
        GesturePhase::Begin => G::Begin,
        GesturePhase::Update => G::Update,
        GesturePhase::End => G::End,
    }
}

fn read_gesture_phase(phase: wire_capnp::GesturePhase) -> GesturePhase {
    use wire_capnp::GesturePhase as G;
    match phase {
        G::Begin => GesturePhase::Begin,
        G::Update => GesturePhase::Update,
        G::End => GesturePhase::End,
    }
}

fn write_menu_items(
    mut list: capnp::struct_list::Builder<wire_capnp::menu_item::Owned>,
    items: &[MenuItem],
) {
    for (i, item) in items.iter().enumerate() {
        let entry = list.reborrow().get(i as u32);
        write_menu_item(entry, item);
    }
}

fn write_menu_item(mut b: wire_capnp::menu_item::Builder, item: &MenuItem) {
    b.set_id(&item.id);
    if let Some(l) = &item.label {
        b.set_label(l);
    }
    b.set_kind(match item.kind {
        MenuItemKind::Normal => wire_capnp::MenuItemKind::Normal,
        MenuItemKind::Submenu => wire_capnp::MenuItemKind::Submenu,
        MenuItemKind::Separator => wire_capnp::MenuItemKind::Separator,
        MenuItemKind::Checkbox => wire_capnp::MenuItemKind::Checkbox,
        MenuItemKind::Radio => wire_capnp::MenuItemKind::Radio,
    });
    b.set_enabled(item.enabled);
    b.set_visible(item.visible);
    b.set_checked(match item.checked {
        None => wire_capnp::CheckState::NotApplicable,
        Some(false) => wire_capnp::CheckState::Unchecked,
        Some(true) => wire_capnp::CheckState::Checked,
    });
    if let Some(a) = &item.accelerator {
        b.set_accelerator(a);
    }
    if let Some(i) = &item.icon {
        b.set_icon(i);
    }
    if let Some(a) = &item.action {
        write_menu_action(b.reborrow().init_action(), a);
    }
    let kids = b.init_children(item.children.len() as u32);
    write_menu_items(kids, &item.children);
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
    let action = if item.has_action() {
        Some(read_menu_action(item.get_action()?)?)
    } else {
        None
    };
    let checked = match item.get_checked()? {
        wire_capnp::CheckState::NotApplicable => None,
        wire_capnp::CheckState::Unchecked => Some(false),
        wire_capnp::CheckState::Checked => Some(true),
    };
    Ok(MenuItem {
        id: item.get_id()?.to_string()?,
        label: if item.has_label() {
            Some(item.get_label()?.to_string()?)
        } else {
            None
        },
        kind,
        enabled: item.get_enabled(),
        visible: item.get_visible(),
        checked,
        accelerator: if item.has_accelerator() {
            Some(item.get_accelerator()?.to_string()?)
        } else {
            None
        },
        icon: if item.has_icon() {
            Some(item.get_icon()?.to_string()?)
        } else {
            None
        },
        action,
        children: read_menu_items(item.get_children()?)?,
    })
}

fn write_menu_action(mut b: wire_capnp::menu_action::Builder, action: &MenuAction) {
    b.set_name(&action.name);
    if let Some(target) = &action.target {
        write_menu_action_target(b.reborrow().init_target(), target);
    }
}

fn write_menu_action_target(
    mut b: wire_capnp::menu_action_target::Builder,
    target: &MenuActionTarget,
) {
    match target {
        MenuActionTarget::String(s) => b.set_string(s),
        MenuActionTarget::Bool(v) => b.set_boolean(*v),
        MenuActionTarget::Int32(v) => b.set_int32(*v),
        MenuActionTarget::UInt32(v) => b.set_u_int32(*v),
        MenuActionTarget::Int64(v) => b.set_int64(*v),
        MenuActionTarget::Float64(v) => b.set_float64(*v),
    }
}

fn read_menu_action(a: wire_capnp::menu_action::Reader) -> Result<MenuAction, BridgeError> {
    let target = if a.has_target() {
        Some(read_menu_action_target(a.get_target()?)?)
    } else {
        None
    };
    Ok(MenuAction {
        name: a.get_name()?.to_string()?,
        target,
    })
}

fn read_menu_action_target(
    r: wire_capnp::menu_action_target::Reader,
) -> Result<MenuActionTarget, BridgeError> {
    use wire_capnp::menu_action_target::Which;
    Ok(match r.which()? {
        Which::String(s) => MenuActionTarget::String(s?.to_string()?),
        Which::Boolean(v) => MenuActionTarget::Bool(v),
        Which::Int32(v) => MenuActionTarget::Int32(v),
        Which::UInt32(v) => MenuActionTarget::UInt32(v),
        Which::Int64(v) => MenuActionTarget::Int64(v),
        Which::Float64(v) => MenuActionTarget::Float64(v),
    })
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

fn read_dnd_event(reader: wire_capnp::dnd_event_kind::Reader) -> Result<DndEventKind, BridgeError> {
    use wire_capnp::dnd_event_kind::Which;
    Ok(match reader.which()? {
        Which::Enter(e) => {
            let e = e?;
            let mts = e.get_mime_types()?;
            let mut out = Vec::with_capacity(mts.len() as usize);
            for entry in mts.iter() {
                out.push(entry?.to_string()?);
            }
            DndEventKind::Enter { mime_types: out }
        }
        Which::Position(p) => {
            let p = p?;
            DndEventKind::Position {
                x: p.get_x(),
                y: p.get_y(),
            }
        }
        Which::Drop(d) => {
            let d = d?;
            DndEventKind::Drop {
                mime_type: d.get_mime_type()?.to_string()?,
                data: d.get_data()?.to_vec(),
            }
        }
        Which::Leave(()) => DndEventKind::Leave,
    })
}
