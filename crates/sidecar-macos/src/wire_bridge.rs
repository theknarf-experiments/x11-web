//! Translation between the macOS sidecar's internal protocol-crate
//! enums (`SidecarToBackend` / `BackendToSidecar` / `DisplayUpdate`
//! / `InputEvent`) and the Cap'n Proto wire types from the `wire`
//! crate.
//!
//! Per-component placement: each side keeps its own bridge so the
//! `wire` crate stays free of dependencies on the protocol crate.
//! Backend has its own copy.
//!
//! Coverage: only the message variants the macOS sidecar actually
//! emits or consumes today. Unsupported variants are skipped on
//! send (logged as a warning) and surfaced as `BridgeError::
//! Unsupported` on receive.

use capnp::message::{Builder, HeapAllocator};
use tracing::warn;
use x11_web_protocol::{BackendToSidecar, DisplayUpdate, InputEvent, SidecarToBackend};
use x11_web_wire::wire_capnp;

#[derive(Debug)]
pub enum BridgeError {
    /// The wire-side message used a variant we haven't translated
    /// to an internal type. Ignore on the recv side (caller may
    /// log) — we don't want unknown new wire variants to crash
    /// older sidecars.
    Unsupported(&'static str),
    /// A capnp read returned an error (decoding etc.).
    Capnp(capnp::Error),
    /// A `Text` field that wasn't valid UTF-8.
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

/// Build a `FromSidecar` message from an internal `SidecarToBackend`.
/// Returns `None` for variants the wire schema doesn't carry
/// (e.g. `Register` is handled at the QUIC handshake, not as a
/// regular message; `RtcOffer` / `RtcIceCandidate` are gone in the
/// new architecture). Caller drops those silently.
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
            // Variants we deliberately skip — handled out-of-band
            // (Register via Hello) or not part of the new
            // architecture (RtcOffer / RtcIceCandidate / Error).
            _ => {
                warn!("wire_bridge: skipping unsupported SidecarToBackend variant");
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
        // Variants we don't (yet) emit from the macOS sidecar.
        // Drawing primitives, cursor events, menu structure, etc.
        _ => {
            warn!("wire_bridge: skipping unsupported DisplayUpdate variant for wire emission");
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Inbound: wire_capnp::ToSidecar → BackendToSidecar
// ---------------------------------------------------------------------------

/// Translate a `ToSidecar` reader into the matching internal
/// `BackendToSidecar` variant. Returns `BridgeError::Unsupported`
/// for variants we don't (yet) honor.
pub fn read_to_sidecar(
    reader: wire_capnp::to_sidecar::Reader,
) -> Result<BackendToSidecar, BridgeError> {
    use wire_capnp::to_sidecar::Which;
    match reader.which()? {
        Which::InputEvent(env) => {
            let env = env?;
            let window_id = env.get_window_id()?.to_string()?;
            let event = read_input_event(env.get_event()?)?;
            Ok(BackendToSidecar::InputEvent { window_id, event })
        }
        Which::RequestRedraw(rr) => {
            let rr = rr?;
            let window_id = rr.get_window_id()?.to_string()?;
            Ok(BackendToSidecar::RequestRedraw { window_id })
        }
        Which::ResizeWindow(rw) => {
            let rw = rw?;
            let window_id = rw.get_window_id()?.to_string()?;
            Ok(BackendToSidecar::ResizeWindow {
                window_id,
                width: rw.get_width(),
                height: rw.get_height(),
            })
        }
        Which::SpawnProcess(_) | Which::KillProcess(_) => {
            // macOS sidecar doesn't honor these yet — schema slot
            // exists for parity with X11.
            Err(BridgeError::Unsupported("spawn/kill not implemented"))
        }
    }
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
    })
}
