//! A ~200-line stand-in for `crates/backend`, for exercising a sidecar
//! end to end without a browser.
//!
//! The compositor has `examples/headless.rs` in `x11-web-wayland-server`
//! for the same reason: without a way to run the thing, the only
//! available claim is "it typechecks". This example is the other half —
//! it proves the parts of the *sidecar* that only exist above the
//! compositor: the `SidecarKind::Wayland` handshake, `SpawnProcess`
//! (and therefore the child's environment), the `/proc`-walk process
//! attribution behind `ProcessConnected`, the WebP encode in
//! `encode_for_wire`, and the `InputEvent` route back down.
//!
//! It is not a backend: no auth, no automerge, no workspaces, one
//! connection then exit. Do not grow it into one.
//!
//! ```text
//! # terminal 1
//! cargo run --example fake_backend -- weston-simple-shm
//! # terminal 2
//! X11WEB_FINGERPRINT_FILE=/tmp/x11web-fake-fingerprint \
//!   BACKEND_QUIC_ADDR=127.0.0.1:3002 x11-web-sidecar-wayland
//! ```
//!
//! Environment: `X11WEB_FAKE_BACKEND_BIND` (default `0.0.0.0:3002`),
//! `X11WEB_FINGERPRINT_FILE` (default
//! `/tmp/x11web-fake-fingerprint`) — the same variable the sidecar
//! reads, so the two only have to agree on one path.

use std::time::Duration;

use x11_web_protocol::{DisplayUpdate, InputEvent};
use x11_web_wire::bridge as wire_bridge;
use x11_web_wire::conn::{accept, listen};
use x11_web_wire::{wire_capnp, BackendToSidecar, SidecarToBackend};

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let bind = std::env::var("X11WEB_FAKE_BACKEND_BIND")
        .unwrap_or_else(|_| "0.0.0.0:3002".into())
        .parse()
        .expect("X11WEB_FAKE_BACKEND_BIND must be host:port");
    let fp_path = std::env::var("X11WEB_FINGERPRINT_FILE")
        .unwrap_or_else(|_| "/tmp/x11web-fake-fingerprint".into());

    let cert = x11_web_wire::tls::generate_self_signed(vec!["localhost".into()])
        .expect("self-signed cert");
    std::fs::write(&fp_path, cert.fingerprint_hex()).expect("write fingerprint");
    println!(
        "FINGERPRINT file={fp_path} value={}",
        cert.fingerprint_hex()
    );

    let endpoint = listen(bind, &cert).expect("bind QUIC listener");
    println!("LISTENING on {bind}");

    // Any token: this is a test harness, and the sidecar's default is
    // `dev-token`.
    let conn = match accept(&endpoint, |_token, name| Ok(format!("fake-{name}"))).await {
        Ok(c) => c,
        Err(e) => {
            println!("FATAL accept: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "SIDECAR name={} kind={:?} id={}",
        conn.sidecar_name, conn.sidecar_kind, conn.sidecar_id
    );

    let (mut reader, mut writer) = (conn.reader, conn.writer);

    if let Some((command, rest)) = args.split_first() {
        let msg = BackendToSidecar::SpawnProcess {
            request_id: "fake-1".into(),
            command: command.clone(),
            args: rest.to_vec(),
        };
        if let Some(b) = wire_bridge::build_to_sidecar(&msg, "") {
            writer.write_message(&b).await.expect("send SpawnProcess");
            println!("SENT SpawnProcess command={command} args={rest:?}");
        }
    }

    // Input is fired once, shortly after the first window maps: early
    // enough to be part of the same run, late enough that the client
    // has drawn and is listening. `38` is the X11 keycode for `a`.
    let mut input_sent = false;
    let mut put_images = 0usize;
    let mut put_image_bytes = 0usize;

    loop {
        let msg = match tokio::time::timeout(
            Duration::from_secs(30),
            reader.read_message::<wire_capnp::from_sidecar::Owned>(),
        )
        .await
        {
            Ok(Ok(Some(m))) => m,
            Ok(Ok(None)) => {
                println!("EOF from sidecar");
                break;
            }
            Ok(Err(e)) => {
                println!("READ ERROR {e}");
                break;
            }
            Err(_) => {
                println!("IDLE 30s, giving up");
                break;
            }
        };
        let root: wire_capnp::from_sidecar::Reader = match msg.get_root() {
            Ok(r) => r,
            Err(e) => {
                println!("ROOT ERROR {e}");
                continue;
            }
        };
        let (decoded, _traceparent) = match wire_bridge::read_from_sidecar(root) {
            Ok(v) => v,
            Err(e) => {
                println!("DECODE ERROR {e:?}");
                continue;
            }
        };

        match decoded {
            SidecarToBackend::Heartbeat => {}
            SidecarToBackend::DisplayUpdate { client_id, update } => match update {
                DisplayUpdate::PutImage {
                    window_id,
                    x,
                    y,
                    width,
                    height,
                    data,
                } => {
                    put_images += 1;
                    put_image_bytes += data.len();
                    // The wire payload is WebP-lossless, so the magic
                    // bytes are the assertion that `encode_for_wire`
                    // actually ran — a raw-RGBA passthrough would look
                    // identical in every other field.
                    let webp = data.len() > 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP";
                    if put_images <= 3 || put_images.is_multiple_of(60) {
                        println!(
                            "PutImage #{put_images} client={client_id} window={window_id} \
                             at={x},{y} size={width}x{height} bytes={} webp={webp}",
                            data.len()
                        );
                    }
                }
                DisplayUpdate::WindowMapped { window_id, .. } => {
                    println!("WindowMapped window={window_id} client={client_id}");
                    if !input_sent {
                        input_sent = true;
                        send_input_burst(&mut writer, &window_id).await;
                    }
                }
                other => println!("{other:?}"),
            },
            other => println!("{other:?}"),
        }
    }

    println!("TOTALS put_images={put_images} put_image_bytes={put_image_bytes}");
}

/// A click and a keystroke on `window_id`, the minimum that proves the
/// backend→sidecar→`WindowRouter`→seat direction is connected.
async fn send_input_burst(writer: &mut x11_web_wire::conn::WireWriter, window_id: &str) {
    let events = [
        InputEvent::MotionNotify {
            x: 20,
            y: 20,
            state: 0,
        },
        InputEvent::ButtonPress {
            button: 1,
            x: 20,
            y: 20,
            state: 0,
        },
        InputEvent::ButtonRelease {
            button: 1,
            x: 20,
            y: 20,
            state: 0,
        },
        InputEvent::KeyPress {
            keycode: 38,
            state: 0,
        },
        InputEvent::KeyRelease {
            keycode: 38,
            state: 0,
        },
    ];
    for event in events {
        let msg = BackendToSidecar::InputEvent {
            window_id: window_id.to_string(),
            event,
        };
        if let Some(b) = wire_bridge::build_to_sidecar(&msg, "") {
            if let Err(e) = writer.write_message(&b).await {
                println!("INPUT WRITE ERROR {e}");
                return;
            }
        }
    }
    println!("SENT input burst (motion, click, key 38) window={window_id}");
}
