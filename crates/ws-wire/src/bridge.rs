//! Translation between the high-level `FrontendToBackend` /
//! `BackendToFrontend` Rust enums (in `x11-web-protocol`) and the
//! Cap'n Proto wire types in `ws_capnp`.
//!
//! Each side of the wire uses two of the four entry points:
//!   * Frontend writes call `encode_frontend_msg`.
//!   * Backend reads call `decode_frontend_msg`.
//!   * Backend writes call `encode_backend_msg`.
//!   * Frontend reads call `decode_backend_msg`.
//!
//! The translation tables mirror `crates/wire/src/bridge.rs` (the
//! sidecar↔backend wire) for shared shapes — InputEvent variants,
//! MenuItem tree, DndEventKind. Schema differences mean we can't
//! literally share the bridge code, but the patterns are identical.

use capnp::message::{Builder, HeapAllocator};
use capnp::serialize;
use x11_web_protocol::{
    BackendToFrontend, DndEventKind, FrontendToBackend, GesturePhase, InputEvent, MenuAction,
    MenuActionTarget, MenuItem, MenuItemKind, ProcessInfo, SidecarInfo, WindowDescriptor,
    WindowUpdate, WindowWmState, Workspace,
};

use crate::ws_capnp;

#[derive(Debug)]
pub enum BridgeError {
    /// Wire-side variant that isn't representable as the high-level
    /// enum (e.g. `noVariant`, which exists only as a forward-compat
    /// fallback for unknown future variants).
    UnknownVariant,
    /// Underlying capnp parse failure (truncated frame, bad layout).
    Capnp(capnp::Error),
    /// Text field contained non-UTF8 bytes.
    Utf8(std::str::Utf8Error),
}

impl From<capnp::Error> for BridgeError {
    fn from(e: capnp::Error) -> Self {
        Self::Capnp(e)
    }
}

impl From<std::str::Utf8Error> for BridgeError {
    fn from(e: std::str::Utf8Error) -> Self {
        Self::Utf8(e)
    }
}

impl From<capnp::NotInSchema> for BridgeError {
    fn from(e: capnp::NotInSchema) -> Self {
        Self::Capnp(capnp::Error::failed(format!("not in schema: {e:?}")))
    }
}

// ============================================================
// Frontend → Backend
// ============================================================

pub fn encode_frontend_msg(msg: &FrontendToBackend, traceparent: &str) -> Vec<u8> {
    let mut builder: Builder<HeapAllocator> = Builder::new_default();
    {
        let mut root = builder.init_root::<ws_capnp::frontend_msg::Builder>();
        root.set_traceparent(traceparent);
        let payload = root.init_payload();
        write_frontend_payload(payload, msg);
    }
    let mut out = Vec::new();
    serialize::write_message(&mut out, &builder).expect("writing to a Vec never fails");
    out
}

fn write_frontend_payload(
    builder: ws_capnp::frontend_msg::payload::Builder,
    msg: &FrontendToBackend,
) {
    match msg {
        FrontendToBackend::OpenWorkspace { id } => {
            let mut ow = builder.init_open_workspace();
            if let Some(id) = id {
                ow.set_id(id);
            }
        }
        FrontendToBackend::SpawnProcess {
            request_id,
            sidecar_id,
            workspace_id,
            command,
            args,
        } => {
            let mut sp = builder.init_spawn_process();
            sp.set_request_id(request_id);
            sp.set_sidecar_id(sidecar_id);
            sp.set_workspace_id(workspace_id);
            sp.set_command(command);
            let mut a = sp.init_args(args.len() as u32);
            for (i, s) in args.iter().enumerate() {
                a.set(i as u32, s);
            }
        }
        FrontendToBackend::KillProcess {
            request_id,
            sidecar_id,
            pid,
        } => {
            let mut kp = builder.init_kill_process();
            kp.set_request_id(request_id);
            kp.set_sidecar_id(sidecar_id);
            kp.set_pid(*pid);
        }
        FrontendToBackend::InputEvent {
            sidecar_id,
            window_id,
            event,
        } => {
            let mut ie = builder.init_input_event();
            ie.set_sidecar_id(sidecar_id);
            ie.set_window_id(window_id);
            write_input_event(ie.init_event(), event);
        }
        FrontendToBackend::ResizeWindow {
            sidecar_id,
            window_id,
            width,
            height,
        } => {
            let mut r = builder.init_resize_window();
            r.set_sidecar_id(sidecar_id);
            r.set_window_id(window_id);
            r.set_width(*width);
            r.set_height(*height);
        }
        FrontendToBackend::RtcOffer { sdp } => {
            let mut o = builder.init_rtc_offer();
            o.set_sdp(sdp);
        }
        FrontendToBackend::RtcIceCandidate {
            candidate,
            sdp_mid,
            sdp_mline_index,
        } => {
            let mut c = builder.init_rtc_ice_candidate();
            c.set_candidate(candidate);
            if let Some(m) = sdp_mid {
                c.set_sdp_mid(m);
            }
            if let Some(idx) = sdp_mline_index {
                c.set_sdp_mline_index_has(true);
                c.set_sdp_mline_index(*idx);
            }
        }
    }
}

pub fn decode_frontend_msg(bytes: &[u8]) -> Result<(FrontendToBackend, String), BridgeError> {
    let reader = serialize::read_message(bytes, capnp::message::ReaderOptions::new())?;
    let root = reader.get_root::<ws_capnp::frontend_msg::Reader>()?;
    let traceparent = root.get_traceparent()?.to_string()?;
    let payload = root.get_payload();
    let msg = read_frontend_payload(payload)?;
    Ok((msg, traceparent))
}

fn read_frontend_payload(
    reader: ws_capnp::frontend_msg::payload::Reader,
) -> Result<FrontendToBackend, BridgeError> {
    use ws_capnp::frontend_msg::payload::Which;
    Ok(match reader.which()? {
        Which::NoVariant(()) => return Err(BridgeError::UnknownVariant),
        Which::OpenWorkspace(r) => {
            let r = r?;
            let id = if r.has_id() {
                Some(r.get_id()?.to_string()?)
            } else {
                None
            };
            FrontendToBackend::OpenWorkspace { id }
        }
        Which::SpawnProcess(r) => {
            let r = r?;
            let args_reader = r.get_args()?;
            let mut args = Vec::with_capacity(args_reader.len() as usize);
            for s in args_reader.iter() {
                args.push(s?.to_string()?);
            }
            FrontendToBackend::SpawnProcess {
                request_id: r.get_request_id()?.to_string()?,
                sidecar_id: r.get_sidecar_id()?.to_string()?,
                workspace_id: r.get_workspace_id()?.to_string()?,
                command: r.get_command()?.to_string()?,
                args,
            }
        }
        Which::KillProcess(r) => {
            let r = r?;
            FrontendToBackend::KillProcess {
                request_id: r.get_request_id()?.to_string()?,
                sidecar_id: r.get_sidecar_id()?.to_string()?,
                pid: r.get_pid(),
            }
        }
        Which::InputEvent(r) => {
            let r = r?;
            FrontendToBackend::InputEvent {
                sidecar_id: r.get_sidecar_id()?.to_string()?,
                window_id: r.get_window_id()?.to_string()?,
                event: read_input_event(r.get_event()?)?,
            }
        }
        Which::ResizeWindow(r) => {
            let r = r?;
            FrontendToBackend::ResizeWindow {
                sidecar_id: r.get_sidecar_id()?.to_string()?,
                window_id: r.get_window_id()?.to_string()?,
                width: r.get_width(),
                height: r.get_height(),
            }
        }
        Which::RtcOffer(r) => {
            let r = r?;
            FrontendToBackend::RtcOffer {
                sdp: r.get_sdp()?.to_string()?,
            }
        }
        Which::RtcIceCandidate(r) => {
            let r = r?;
            FrontendToBackend::RtcIceCandidate {
                candidate: r.get_candidate()?.to_string()?,
                sdp_mid: if r.has_sdp_mid() {
                    Some(r.get_sdp_mid()?.to_string()?)
                } else {
                    None
                },
                sdp_mline_index: if r.get_sdp_mline_index_has() {
                    Some(r.get_sdp_mline_index())
                } else {
                    None
                },
            }
        }
    })
}

// ============================================================
// Backend → Frontend
// ============================================================

pub fn encode_backend_msg(msg: &BackendToFrontend, traceparent: &str) -> Vec<u8> {
    let mut builder: Builder<HeapAllocator> = Builder::new_default();
    {
        let mut root = builder.init_root::<ws_capnp::backend_msg::Builder>();
        root.set_traceparent(traceparent);
        let payload = root.init_payload();
        write_backend_payload(payload, msg);
    }
    let mut out = Vec::new();
    serialize::write_message(&mut out, &builder).expect("writing to a Vec never fails");
    out
}

fn write_backend_payload(
    builder: ws_capnp::backend_msg::payload::Builder,
    msg: &BackendToFrontend,
) {
    match msg {
        BackendToFrontend::SidecarList { sidecars } => {
            let sl = builder.init_sidecar_list();
            let mut list = sl.init_sidecars(sidecars.len() as u32);
            for (i, s) in sidecars.iter().enumerate() {
                let mut e = list.reborrow().get(i as u32);
                e.set_id(&s.id);
                e.set_name(&s.name);
            }
        }
        BackendToFrontend::Workspace { workspace } => {
            let mut w = builder.init_workspace().init_workspace();
            w.set_id(&workspace.id);
            w.set_name(&workspace.name);
        }
        BackendToFrontend::CommandResult {
            request_id,
            success,
            message,
        } => {
            let mut c = builder.init_command_result();
            c.set_request_id(request_id);
            c.set_success(*success);
            c.set_message(message);
        }
        BackendToFrontend::ProcessList {
            sidecar_id,
            processes,
        } => {
            let mut p = builder.init_process_list();
            p.set_sidecar_id(sidecar_id);
            let mut list = p.init_processes(processes.len() as u32);
            for (i, pi) in processes.iter().enumerate() {
                let mut e = list.reborrow().get(i as u32);
                e.set_pid(pi.pid);
                e.set_client_id(&pi.client_id);
                e.set_command(&pi.command);
            }
        }
        BackendToFrontend::WindowUpdate { update } => {
            let wu = builder.init_window_update().init_update();
            write_window_update(wu, update);
        }
        BackendToFrontend::WindowList { windows } => {
            let wl = builder.init_window_list();
            let mut list = wl.init_windows(windows.len() as u32);
            for (i, w) in windows.iter().enumerate() {
                write_window_descriptor(list.reborrow().get(i as u32), w);
            }
        }
        BackendToFrontend::Bell { percent } => {
            builder.init_bell().set_percent(*percent);
        }
        BackendToFrontend::RtcAnswer { sdp } => {
            builder.init_rtc_answer().set_sdp(sdp);
        }
        BackendToFrontend::RtcIceCandidate {
            candidate,
            sdp_mid,
            sdp_mline_index,
        } => {
            let mut c = builder.init_rtc_ice_candidate();
            c.set_candidate(candidate);
            if let Some(m) = sdp_mid {
                c.set_sdp_mid(m);
            }
            if let Some(idx) = sdp_mline_index {
                c.set_sdp_mline_index_has(true);
                c.set_sdp_mline_index(*idx);
            }
        }
    }
}

pub fn decode_backend_msg(bytes: &[u8]) -> Result<(BackendToFrontend, String), BridgeError> {
    let reader = serialize::read_message(bytes, capnp::message::ReaderOptions::new())?;
    let root = reader.get_root::<ws_capnp::backend_msg::Reader>()?;
    let traceparent = root.get_traceparent()?.to_string()?;
    let payload = root.get_payload();
    let msg = read_backend_payload(payload)?;
    Ok((msg, traceparent))
}

fn read_backend_payload(
    reader: ws_capnp::backend_msg::payload::Reader,
) -> Result<BackendToFrontend, BridgeError> {
    use ws_capnp::backend_msg::payload::Which;
    Ok(match reader.which()? {
        Which::NoVariant(()) => return Err(BridgeError::UnknownVariant),
        Which::SidecarList(r) => {
            let r = r?;
            let list = r.get_sidecars()?;
            let mut sidecars = Vec::with_capacity(list.len() as usize);
            for e in list.iter() {
                sidecars.push(SidecarInfo {
                    id: e.get_id()?.to_string()?,
                    name: e.get_name()?.to_string()?,
                });
            }
            BackendToFrontend::SidecarList { sidecars }
        }
        Which::Workspace(r) => {
            let r = r?;
            let w = r.get_workspace()?;
            BackendToFrontend::Workspace {
                workspace: Workspace {
                    id: w.get_id()?.to_string()?,
                    name: w.get_name()?.to_string()?,
                },
            }
        }
        Which::CommandResult(r) => {
            let r = r?;
            BackendToFrontend::CommandResult {
                request_id: r.get_request_id()?.to_string()?,
                success: r.get_success(),
                message: r.get_message()?.to_string()?,
            }
        }
        Which::ProcessList(r) => {
            let r = r?;
            let list = r.get_processes()?;
            let mut processes = Vec::with_capacity(list.len() as usize);
            for e in list.iter() {
                processes.push(ProcessInfo {
                    pid: e.get_pid(),
                    client_id: e.get_client_id()?.to_string()?,
                    command: e.get_command()?.to_string()?,
                });
            }
            BackendToFrontend::ProcessList {
                sidecar_id: r.get_sidecar_id()?.to_string()?,
                processes,
            }
        }
        Which::WindowUpdate(r) => {
            let r = r?;
            BackendToFrontend::WindowUpdate {
                update: read_window_update(r.get_update()?)?,
            }
        }
        Which::WindowList(r) => {
            let r = r?;
            let list = r.get_windows()?;
            let mut windows = Vec::with_capacity(list.len() as usize);
            for e in list.iter() {
                windows.push(read_window_descriptor(e)?);
            }
            BackendToFrontend::WindowList { windows }
        }
        Which::Bell(r) => {
            let r = r?;
            BackendToFrontend::Bell {
                percent: r.get_percent(),
            }
        }
        Which::RtcAnswer(r) => {
            let r = r?;
            BackendToFrontend::RtcAnswer {
                sdp: r.get_sdp()?.to_string()?,
            }
        }
        Which::RtcIceCandidate(r) => {
            let r = r?;
            BackendToFrontend::RtcIceCandidate {
                candidate: r.get_candidate()?.to_string()?,
                sdp_mid: if r.has_sdp_mid() {
                    Some(r.get_sdp_mid()?.to_string()?)
                } else {
                    None
                },
                sdp_mline_index: if r.get_sdp_mline_index_has() {
                    Some(r.get_sdp_mline_index())
                } else {
                    None
                },
            }
        }
    })
}

// ============================================================
// Shared helpers
// ============================================================

fn write_window_descriptor(mut b: ws_capnp::window_descriptor::Builder, w: &WindowDescriptor) {
    b.set_window_id(&w.window_id);
    b.set_sidecar_id(&w.sidecar_id);
    b.set_pid(w.pid);
    b.set_command(&w.command);
    b.set_x(w.x);
    b.set_y(w.y);
    b.set_width(w.width);
    b.set_height(w.height);
    b.set_border_width(w.border_width);
    b.set_border_pixel(w.border_pixel);
    b.set_override_redirect(w.override_redirect);
    b.set_resizable(w.resizable);
}

fn read_window_descriptor(
    r: ws_capnp::window_descriptor::Reader,
) -> Result<WindowDescriptor, BridgeError> {
    Ok(WindowDescriptor {
        window_id: r.get_window_id()?.to_string()?,
        sidecar_id: r.get_sidecar_id()?.to_string()?,
        pid: r.get_pid(),
        command: r.get_command()?.to_string()?,
        x: r.get_x(),
        y: r.get_y(),
        width: r.get_width(),
        height: r.get_height(),
        border_width: r.get_border_width(),
        border_pixel: r.get_border_pixel(),
        override_redirect: r.get_override_redirect(),
        resizable: r.get_resizable(),
    })
}

fn write_window_update(builder: ws_capnp::window_update::Builder, update: &WindowUpdate) {
    match update {
        WindowUpdate::TitleChanged { window_id, title } => {
            let mut t = builder.init_title_changed();
            t.set_window_id(window_id);
            t.set_title(title);
        }
        WindowUpdate::StateChanged { window_id, state } => {
            let mut s = builder.init_state_changed();
            s.set_window_id(window_id);
            s.set_state(write_wm_state(*state));
        }
        WindowUpdate::Focused { window_id } => {
            let mut f = builder.init_focused();
            if let Some(id) = window_id {
                f.set_window_id(id);
            }
        }
        WindowUpdate::MenuStructure { window_id, menu } => {
            let mut m = builder.init_menu_structure();
            m.set_window_id(window_id);
            let items = m.init_items(menu.len() as u32);
            write_menu_items(items, menu);
        }
    }
}

fn read_window_update(
    reader: ws_capnp::window_update::Reader,
) -> Result<WindowUpdate, BridgeError> {
    use ws_capnp::window_update::Which;
    Ok(match reader.which()? {
        Which::NoVariant(()) => return Err(BridgeError::UnknownVariant),
        Which::TitleChanged(r) => {
            let r = r?;
            WindowUpdate::TitleChanged {
                window_id: r.get_window_id()?.to_string()?,
                title: r.get_title()?.to_string()?,
            }
        }
        Which::StateChanged(r) => {
            let r = r?;
            WindowUpdate::StateChanged {
                window_id: r.get_window_id()?.to_string()?,
                state: read_wm_state(r.get_state()?),
            }
        }
        Which::Focused(r) => {
            let r = r?;
            WindowUpdate::Focused {
                window_id: if r.has_window_id() {
                    Some(r.get_window_id()?.to_string()?)
                } else {
                    None
                },
            }
        }
        Which::MenuStructure(r) => {
            let r = r?;
            WindowUpdate::MenuStructure {
                window_id: r.get_window_id()?.to_string()?,
                menu: read_menu_items(r.get_items()?)?,
            }
        }
    })
}

fn write_wm_state(state: WindowWmState) -> ws_capnp::WindowWmState {
    use ws_capnp::WindowWmState as W;
    match state {
        WindowWmState::Normal => W::Normal,
        WindowWmState::Minimized => W::Minimized,
        WindowWmState::Maximized => W::Maximized,
        WindowWmState::Fullscreen => W::Fullscreen,
        WindowWmState::Close => W::Close,
    }
}

fn read_wm_state(state: ws_capnp::WindowWmState) -> WindowWmState {
    use ws_capnp::WindowWmState as W;
    match state {
        W::Normal => WindowWmState::Normal,
        W::Minimized => WindowWmState::Minimized,
        W::Maximized => WindowWmState::Maximized,
        W::Fullscreen => WindowWmState::Fullscreen,
        W::Close => WindowWmState::Close,
    }
}

fn write_gesture_phase(phase: &GesturePhase) -> ws_capnp::GesturePhase {
    use ws_capnp::GesturePhase as G;
    match phase {
        GesturePhase::Begin => G::Begin,
        GesturePhase::Update => G::Update,
        GesturePhase::End => G::End,
    }
}

fn read_gesture_phase(phase: ws_capnp::GesturePhase) -> GesturePhase {
    use ws_capnp::GesturePhase as G;
    match phase {
        G::Begin => GesturePhase::Begin,
        G::Update => GesturePhase::Update,
        G::End => GesturePhase::End,
    }
}

fn write_menu_items(
    mut list: capnp::struct_list::Builder<ws_capnp::menu_item::Owned>,
    items: &[MenuItem],
) {
    for (i, item) in items.iter().enumerate() {
        write_menu_item(list.reborrow().get(i as u32), item);
    }
}

fn write_menu_item(mut b: ws_capnp::menu_item::Builder, item: &MenuItem) {
    b.set_id(&item.id);
    if let Some(l) = &item.label {
        b.set_label(l);
    }
    b.set_kind(match item.kind {
        MenuItemKind::Normal => ws_capnp::MenuItemKind::Normal,
        MenuItemKind::Submenu => ws_capnp::MenuItemKind::Submenu,
        MenuItemKind::Separator => ws_capnp::MenuItemKind::Separator,
        MenuItemKind::Checkbox => ws_capnp::MenuItemKind::Checkbox,
        MenuItemKind::Radio => ws_capnp::MenuItemKind::Radio,
    });
    b.set_enabled(item.enabled);
    b.set_visible(item.visible);
    b.set_checked(match item.checked {
        None => ws_capnp::CheckState::NotApplicable,
        Some(false) => ws_capnp::CheckState::Unchecked,
        Some(true) => ws_capnp::CheckState::Checked,
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
    list: capnp::struct_list::Reader<ws_capnp::menu_item::Owned>,
) -> Result<Vec<MenuItem>, BridgeError> {
    let mut out = Vec::with_capacity(list.len() as usize);
    for entry in list.iter() {
        out.push(read_menu_item(entry)?);
    }
    Ok(out)
}

fn read_menu_item(item: ws_capnp::menu_item::Reader) -> Result<MenuItem, BridgeError> {
    let kind = match item.get_kind()? {
        ws_capnp::MenuItemKind::Normal => MenuItemKind::Normal,
        ws_capnp::MenuItemKind::Submenu => MenuItemKind::Submenu,
        ws_capnp::MenuItemKind::Separator => MenuItemKind::Separator,
        ws_capnp::MenuItemKind::Checkbox => MenuItemKind::Checkbox,
        ws_capnp::MenuItemKind::Radio => MenuItemKind::Radio,
    };
    let action = if item.has_action() {
        Some(read_menu_action(item.get_action()?)?)
    } else {
        None
    };
    let checked = match item.get_checked()? {
        ws_capnp::CheckState::NotApplicable => None,
        ws_capnp::CheckState::Unchecked => Some(false),
        ws_capnp::CheckState::Checked => Some(true),
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

fn write_menu_action(mut b: ws_capnp::menu_action::Builder, action: &MenuAction) {
    b.set_name(&action.name);
    if let Some(target) = &action.target {
        write_menu_action_target(b.reborrow().init_target(), target);
    }
}

fn write_menu_action_target(
    mut b: ws_capnp::menu_action_target::Builder,
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

fn read_menu_action(a: ws_capnp::menu_action::Reader) -> Result<MenuAction, BridgeError> {
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
    r: ws_capnp::menu_action_target::Reader,
) -> Result<MenuActionTarget, BridgeError> {
    use ws_capnp::menu_action_target::Which;
    Ok(match r.which()? {
        Which::String(s) => MenuActionTarget::String(s?.to_string()?),
        Which::Boolean(v) => MenuActionTarget::Bool(v),
        Which::Int32(v) => MenuActionTarget::Int32(v),
        Which::UInt32(v) => MenuActionTarget::UInt32(v),
        Which::Int64(v) => MenuActionTarget::Int64(v),
        Which::Float64(v) => MenuActionTarget::Float64(v),
    })
}

// InputEvent translation — mirror of the same shape in
// `crates/wire/src/bridge.rs`. Schema layout is identical except
// for the named-union wrapper (`payload :union` here).

fn write_input_event(builder: ws_capnp::input_event::Builder, event: &InputEvent) {
    let payload = builder.init_payload();
    match event {
        InputEvent::KeyPress { keycode, state } => {
            let mut kp = payload.init_key_press();
            kp.set_keycode(*keycode);
            kp.set_state(*state);
        }
        InputEvent::KeyRelease { keycode, state } => {
            let mut kr = payload.init_key_release();
            kr.set_keycode(*keycode);
            kr.set_state(*state);
        }
        InputEvent::ButtonPress {
            button,
            x,
            y,
            state,
        } => {
            let mut bp = payload.init_button_press();
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
            let mut br = payload.init_button_release();
            br.set_button(*button);
            br.set_x(*x);
            br.set_y(*y);
            br.set_state(*state);
        }
        InputEvent::MotionNotify { x, y, state } => {
            let mut mn = payload.init_motion_notify();
            mn.set_x(*x);
            mn.set_y(*y);
            mn.set_state(*state);
        }
        InputEvent::MenuActivate { action } => {
            let ma = payload.init_menu_activate();
            write_menu_action(ma.init_action(), action);
        }
        InputEvent::WindowManage { action } => {
            let mut wm = payload.init_window_manage();
            wm.set_action(write_wm_state(*action));
        }
        InputEvent::DndBridge { event } => {
            let db = payload.init_dnd_bridge();
            write_dnd_event(db.init_event(), event);
        }
        InputEvent::TouchBegin {
            touch_id,
            x,
            y,
            state,
        } => {
            let mut t = payload.init_touch_begin();
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
            let mut t = payload.init_touch_update();
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
            let mut t = payload.init_touch_end();
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
            let mut gs = payload.init_gesture_swipe();
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
            let mut gp = payload.init_gesture_pinch();
            gp.set_phase(write_gesture_phase(phase));
            gp.set_fingers(*fingers);
            gp.set_dx(*dx);
            gp.set_dy(*dy);
            gp.set_scale(*scale);
            gp.set_rotation(*rotation);
        }
        InputEvent::CompositionEvent { phase, text } => {
            let mut ce = payload.init_composition_event();
            ce.set_phase(phase);
            ce.set_text(text);
        }
    }
}

fn read_input_event(reader: ws_capnp::input_event::Reader) -> Result<InputEvent, BridgeError> {
    use ws_capnp::input_event::payload::Which;
    let payload = reader.get_payload();
    Ok(match payload.which()? {
        Which::NoVariant(()) => return Err(BridgeError::UnknownVariant),
        Which::KeyPress(r) => {
            let r = r?;
            InputEvent::KeyPress {
                keycode: r.get_keycode(),
                state: r.get_state(),
            }
        }
        Which::KeyRelease(r) => {
            let r = r?;
            InputEvent::KeyRelease {
                keycode: r.get_keycode(),
                state: r.get_state(),
            }
        }
        Which::ButtonPress(r) => {
            let r = r?;
            InputEvent::ButtonPress {
                button: r.get_button(),
                x: r.get_x(),
                y: r.get_y(),
                state: r.get_state(),
            }
        }
        Which::ButtonRelease(r) => {
            let r = r?;
            InputEvent::ButtonRelease {
                button: r.get_button(),
                x: r.get_x(),
                y: r.get_y(),
                state: r.get_state(),
            }
        }
        Which::MotionNotify(r) => {
            let r = r?;
            InputEvent::MotionNotify {
                x: r.get_x(),
                y: r.get_y(),
                state: r.get_state(),
            }
        }
        Which::MenuActivate(r) => {
            let r = r?;
            InputEvent::MenuActivate {
                action: read_menu_action(r.get_action()?)?,
            }
        }
        Which::WindowManage(r) => {
            let r = r?;
            InputEvent::WindowManage {
                action: read_wm_state(r.get_action()?),
            }
        }
        Which::DndBridge(r) => {
            let r = r?;
            InputEvent::DndBridge {
                event: read_dnd_event(r.get_event()?)?,
            }
        }
        Which::TouchBegin(r) => {
            let r = r?;
            InputEvent::TouchBegin {
                touch_id: r.get_touch_id(),
                x: r.get_x(),
                y: r.get_y(),
                state: r.get_state(),
            }
        }
        Which::TouchUpdate(r) => {
            let r = r?;
            InputEvent::TouchUpdate {
                touch_id: r.get_touch_id(),
                x: r.get_x(),
                y: r.get_y(),
                state: r.get_state(),
            }
        }
        Which::TouchEnd(r) => {
            let r = r?;
            InputEvent::TouchEnd {
                touch_id: r.get_touch_id(),
                x: r.get_x(),
                y: r.get_y(),
                state: r.get_state(),
            }
        }
        Which::GestureSwipe(r) => {
            let r = r?;
            InputEvent::GestureSwipe {
                phase: read_gesture_phase(r.get_phase()?),
                fingers: r.get_fingers(),
                dx: r.get_dx(),
                dy: r.get_dy(),
            }
        }
        Which::GesturePinch(r) => {
            let r = r?;
            InputEvent::GesturePinch {
                phase: read_gesture_phase(r.get_phase()?),
                fingers: r.get_fingers(),
                dx: r.get_dx(),
                dy: r.get_dy(),
                scale: r.get_scale(),
                rotation: r.get_rotation(),
            }
        }
        Which::CompositionEvent(r) => {
            let r = r?;
            InputEvent::CompositionEvent {
                phase: r.get_phase()?.to_string()?,
                text: r.get_text()?.to_string()?,
            }
        }
    })
}

fn write_dnd_event(builder: ws_capnp::dnd_event::Builder, event: &DndEventKind) {
    let payload = builder.init_payload();
    match event {
        DndEventKind::Enter { mime_types } => {
            let enter = payload.init_enter();
            let mut list = enter.init_mime_types(mime_types.len() as u32);
            for (i, mt) in mime_types.iter().enumerate() {
                list.set(i as u32, mt);
            }
        }
        DndEventKind::Position { x, y } => {
            let mut p = payload.init_position();
            p.set_x(*x);
            p.set_y(*y);
        }
        DndEventKind::Drop { mime_type, data } => {
            let mut d = payload.init_drop();
            d.set_mime_type(mime_type);
            d.set_data(data);
        }
        DndEventKind::Leave => {
            let mut payload = payload;
            payload.set_leave(());
        }
    }
}

fn read_dnd_event(reader: ws_capnp::dnd_event::Reader) -> Result<DndEventKind, BridgeError> {
    use ws_capnp::dnd_event::payload::Which;
    let payload = reader.get_payload();
    Ok(match payload.which()? {
        Which::NoVariant(()) => return Err(BridgeError::UnknownVariant),
        Which::Enter(r) => {
            let r = r?;
            let mts = r.get_mime_types()?;
            let mut out = Vec::with_capacity(mts.len() as usize);
            for entry in mts.iter() {
                out.push(entry?.to_string()?);
            }
            DndEventKind::Enter { mime_types: out }
        }
        Which::Position(r) => {
            let r = r?;
            DndEventKind::Position {
                x: r.get_x(),
                y: r.get_y(),
            }
        }
        Which::Drop(r) => {
            let r = r?;
            DndEventKind::Drop {
                mime_type: r.get_mime_type()?.to_string()?,
                data: r.get_data()?.to_vec(),
            }
        }
        Which::Leave(()) => DndEventKind::Leave,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_roundtrip_spawn_process() {
        let original = FrontendToBackend::SpawnProcess {
            request_id: "req-123".into(),
            sidecar_id: "sc-abc".into(),
            workspace_id: "ws-xyz".into(),
            command: "xeyes".into(),
            args: vec!["-bg".into(), "yellow".into()],
        };
        let bytes = encode_frontend_msg(&original, "tp-1");
        let (decoded, tp) = decode_frontend_msg(&bytes).expect("decode");
        assert_eq!(tp, "tp-1");
        match decoded {
            FrontendToBackend::SpawnProcess {
                request_id,
                sidecar_id,
                workspace_id,
                command,
                args,
            } => {
                assert_eq!(request_id, "req-123");
                assert_eq!(sidecar_id, "sc-abc");
                assert_eq!(workspace_id, "ws-xyz");
                assert_eq!(command, "xeyes");
                assert_eq!(args, vec!["-bg".to_string(), "yellow".into()]);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn frontend_roundtrip_input_button_press() {
        let original = FrontendToBackend::InputEvent {
            sidecar_id: "sc-1".into(),
            window_id: "win-1".into(),
            event: InputEvent::ButtonPress {
                button: 1,
                x: 10,
                y: 20,
                state: 0,
            },
        };
        let bytes = encode_frontend_msg(&original, "");
        let (decoded, _) = decode_frontend_msg(&bytes).expect("decode");
        match decoded {
            FrontendToBackend::InputEvent { event, .. } => match event {
                InputEvent::ButtonPress {
                    button,
                    x,
                    y,
                    state,
                } => {
                    assert_eq!((button, x, y, state), (1, 10, 20, 0));
                }
                other => panic!("wrong event: {other:?}"),
            },
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn backend_roundtrip_window_list() {
        let original = BackendToFrontend::WindowList {
            windows: vec![WindowDescriptor {
                window_id: "w1".into(),
                sidecar_id: "s1".into(),
                pid: 42,
                command: "xterm".into(),
                x: 10.5,
                y: -20.0,
                width: 800,
                height: 600,
                border_width: 1,
                border_pixel: 0xff00ff,
                override_redirect: false,
                resizable: true,
            }],
        };
        let bytes = encode_backend_msg(&original, "tp-2");
        let (decoded, tp) = decode_backend_msg(&bytes).expect("decode");
        assert_eq!(tp, "tp-2");
        match decoded {
            BackendToFrontend::WindowList { windows } => {
                assert_eq!(windows.len(), 1);
                let w = &windows[0];
                assert_eq!(w.window_id, "w1");
                assert_eq!(w.pid, 42);
                assert!((w.x - 10.5).abs() < f64::EPSILON);
                assert_eq!(w.width, 800);
                assert!(w.resizable);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }
}
