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
use x11_web_protocol::{BackendToSidecar, DisplayUpdate, InputEvent, SidecarToBackend};
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
            // Variants the wire schema deliberately doesn't carry
            // (clipboard, RandR, RTC, list-processes). Drop with
            // a log so a future schema addition isn't silently
            // missing.
            _ => {
                warn!("wire_bridge: skipping unsupported BackendToSidecar variant");
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
        // Touch / gesture / WindowManage / DndBridge / etc. — not
        // in the wire schema for this round.
        _ => {
            warn!("wire_bridge: skipping unsupported InputEvent variant");
            return false;
        }
    }
    true
}
