//! Translation between the X11 sidecar's internal protocol-crate
//! enums and the Cap'n Proto wire types from the `wire` crate.
//!
//! Mirror of `crates/backend/src/wire_bridge.rs` — same translation
//! tables, opposite direction. The X11 sidecar:
//!   - **Sends** `SidecarToBackend` → builds `FromSidecar`.
//!   - **Receives** `ToSidecar` → translates to `BackendToSidecar`.
//!
//! Per-component placement (rather than centralising in the wire
//! crate) keeps the wire crate free of the protocol crate's serde
//! types and lets each sidecar implementation pick which variants
//! it actually emits or consumes.

use capnp::message::{Builder, HeapAllocator};
use tracing::warn;
use x11_web_protocol::{
    AnimCursorFrame, BackendToSidecar, DisplayUpdate, DndEventKind, GesturePhase, InputEvent,
    MenuAction, MenuItem, MenuItemKind, SidecarToBackend, WindowWmState,
};
use x11_web_wire::wire_capnp;

#[derive(Debug)]
pub enum BridgeError {
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
                match exit_code {
                    Some(code) => {
                        pe.set_has_exit_code(true);
                        pe.set_exit_code(*code);
                    }
                    None => {
                        pe.set_has_exit_code(false);
                        pe.set_exit_code(0);
                    }
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
                match request_id {
                    Some(id) => {
                        er.set_has_request_id(true);
                        er.set_request_id(id);
                    }
                    None => {
                        er.set_has_request_id(false);
                    }
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
            // Handshake-only and architecturally-removed variants.
            SidecarToBackend::Register { .. }
            | SidecarToBackend::RtcOffer { .. }
            | SidecarToBackend::RtcIceCandidate { .. } => {
                return None;
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
            match window_id {
                Some(id) => {
                    wf.set_has_window_id(true);
                    wf.set_window_id(id);
                }
                None => {
                    wf.set_has_window_id(false);
                }
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
        DisplayUpdate::WindowUrgent { window_id, urgent } => {
            let mut wu = payload.init_window_urgent();
            wu.set_window_id(window_id);
            wu.set_urgent(*urgent);
        }
        DisplayUpdate::TransientForSet {
            window_id,
            parent_window_id,
        } => {
            let mut tfs = payload.init_transient_for_set();
            tfs.set_window_id(window_id);
            match parent_window_id {
                Some(p) => {
                    tfs.set_has_parent(true);
                    tfs.set_parent_window_id(p);
                }
                None => {
                    tfs.set_has_parent(false);
                }
            }
        }
        DisplayUpdate::WindowIconChanged {
            window_id,
            width,
            height,
            data,
        } => {
            let mut wic = payload.init_window_icon_changed();
            wic.set_window_id(window_id);
            wic.set_width(*width);
            wic.set_height(*height);
            wic.set_data(data);
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
        DisplayUpdate::MenuStateChanged {
            window_id,
            item_id,
            enabled,
            checked,
            label,
        } => {
            let mut ms = payload.init_menu_state_changed();
            ms.set_window_id(window_id);
            ms.set_item_id(item_id);
            match enabled {
                Some(v) => {
                    ms.set_has_enabled(true);
                    ms.set_enabled(*v);
                }
                None => ms.set_has_enabled(false),
            }
            match checked {
                Some(v) => {
                    ms.set_has_checked(true);
                    ms.set_checked(*v);
                }
                None => ms.set_has_checked(false),
            }
            match label {
                Some(v) => {
                    ms.set_has_label(true);
                    ms.set_label(v);
                }
                None => ms.set_has_label(false),
            }
        }
        // Drawing primitives, clipboard-as-display-update,
        // ClearArea, CopyArea, DrawArc, DrawLines, FillRect,
        // CursorConfined, DndEvent — variants the X11 sidecar's
        // current emitter set doesn't produce. We log + skip; if
        // something starts emitting one of these the warn surfaces
        // immediately.
        _ => {
            warn!("wire_bridge: skipping unsupported DisplayUpdate variant for wire emission");
            return false;
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
    match &item.label {
        Some(l) => {
            b.set_has_label(true);
            b.set_label(l);
        }
        None => b.set_has_label(false),
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
    match item.checked {
        Some(v) => {
            b.set_has_checked(true);
            b.set_checked(v);
        }
        None => b.set_has_checked(false),
    }
    match &item.accelerator {
        Some(a) => {
            b.set_has_accelerator(true);
            b.set_accelerator(a);
        }
        None => b.set_has_accelerator(false),
    }
    match &item.icon {
        Some(i) => {
            b.set_has_icon(true);
            b.set_icon(i);
        }
        None => b.set_has_icon(false),
    }
    match &item.action {
        Some(a) => {
            b.set_has_action(true);
            write_menu_action(b.reborrow().init_action(), a);
        }
        None => b.set_has_action(false),
    }
    let kids = b.init_children(item.children.len() as u32);
    write_menu_items(kids, &item.children);
}

fn write_menu_action(mut b: wire_capnp::menu_action::Builder, action: &MenuAction) {
    b.set_name(&action.name);
    if let Some(target) = &action.target {
        b.set_has_target(true);
        let s = serde_json::to_string(target).unwrap_or_else(|_| "null".into());
        b.set_target_json(&s);
    } else {
        b.set_has_target(false);
    }
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

fn read_gesture_phase(phase: wire_capnp::GesturePhase) -> GesturePhase {
    use wire_capnp::GesturePhase as G;
    match phase {
        G::Begin => GesturePhase::Begin,
        G::Update => GesturePhase::Update,
        G::End => GesturePhase::End,
    }
}

fn read_menu_action(a: wire_capnp::menu_action::Reader) -> Result<MenuAction, BridgeError> {
    let target = if a.get_has_target() {
        let txt = a.get_target_json()?.to_string()?;
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
