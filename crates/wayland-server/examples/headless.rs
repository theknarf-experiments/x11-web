//! Run the compositor on its own, print every `DisplayUpdate` it
//! emits, and inject input read from stdin.
//!
//! This is the smallest possible embedder: it does what
//! `x11-web-sidecar-wayland` does minus the QUIC wire, so the
//! compositor can be exercised against a real Wayland client with no
//! backend, no frontend and no browser in the loop:
//!
//! ```text
//! cargo run --example headless          # prints WAYLAND_DISPLAY=…
//! WAYLAND_DISPLAY=wayland-1 weston-simple-shm
//! ```
//!
//! It is also the harness the packaging stage's container smoke tests
//! drive, which is why it prints machine-greppable lines rather than
//! going through `tracing`.
//!
//! ## Input
//!
//! Stdin is a line-oriented command language standing in for the
//! browser. It exists because there is otherwise no way to prove the
//! seat works before the sidecar and the frontend are both wired up —
//! the alternative is shipping input on a typecheck, which is exactly
//! how a `kb_active` that is never set gets missed.
//!
//! Commands address the most recently mapped window unless `window` is
//! used to pick another. `<mask>` is the X11 `KeyButMask` the frontend
//! sends (0x01 shift, 0x04 ctrl, 0x08 alt, 0x40 super) and defaults to
//! 0.
//!
//! ```text
//! motion <x> <y> [mask]
//! button <n> down|up <x> <y> [mask]     # 1=left 2=middle 3=right, 4-7=wheel
//! key <x11-keycode> down|up [mask]      # e.g. 38 = 'a'
//! manage close|minimize|maximize|normal|fullscreen
//! resize <w> <h>
//! window <uuid>
//! ```

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("the wayland compositor only builds on Linux; run this under tools/wayland-build.sh");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn main() {
    use std::io::BufRead;
    use std::time::Duration;
    use x11_web_protocol::{DisplayUpdate, InputEvent, WindowWmState};
    use x11_web_wayland_server::{WaylandServer, WindowRouter};

    let (update_tx, mut update_rx) = tokio::sync::mpsc::unbounded_channel();
    let (client_tx, mut client_rx) = tokio::sync::mpsc::unbounded_channel();
    let router = WindowRouter::new();

    let server = match WaylandServer::new(
        update_tx,
        client_tx,
        router.clone(),
        (1280, 800),
        Duration::from_millis(16),
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("FATAL: {e}");
            std::process::exit(1);
        }
    };

    println!("WAYLAND_DISPLAY={}", server.wayland_display_name());
    println!("XDG_RUNTIME_DIR={}", server.xdg_runtime_dir().display());
    println!("SOCKET={}", server.socket_path().display());

    // Stdin on its own thread: the main loop must keep draining the
    // update channel while a smoke script pauses between commands, and
    // a blocking read here would stall it.
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if cmd_tx.send(line).is_err() {
                return;
            }
        }
    });

    let mut target: Option<String> = None;

    // No tokio runtime on purpose: `try_recv` works off-runtime, and
    // keeping the example single-threaded and synchronous means what
    // it proves is the compositor, not the async plumbing around it.
    loop {
        while let Ok((client_id, pid)) = client_rx.try_recv() {
            println!("CLIENT client_id={client_id} pid={pid}");
        }
        while let Ok((client_id, update)) = update_rx.try_recv() {
            // Address the most recently mapped window by default, so a
            // script that starts one client can go straight to sending
            // input without parsing UUIDs out of the log.
            if let DisplayUpdate::WindowMapped { window_id, .. } = &update {
                target = Some(window_id.clone());
            }
            match update {
                DisplayUpdate::PutImage {
                    window_id,
                    x,
                    y,
                    width,
                    height,
                    data,
                } => {
                    let non_black = data
                        .chunks_exact(4)
                        .filter(|p| p[0] > 8 || p[1] > 8 || p[2] > 8)
                        .count();
                    println!(
                        "PutImage client={client_id} window={window_id} at={x},{y} \
                         size={width}x{height} bytes={} non_black_px={non_black} \
                         first_px={:?}",
                        data.len(),
                        &data[..data.len().min(4)]
                    );
                }
                other => println!("{other:?}"),
            }
        }

        while let Ok(line) = cmd_rx.try_recv() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.is_empty() || f[0].starts_with('#') {
                continue;
            }
            let num = |i: usize| -> i64 { f.get(i).and_then(|s| parse_int(s)).unwrap_or(0) };
            let mask =
                |i: usize| -> u16 { f.get(i).and_then(|s| parse_int(s)).unwrap_or(0) as u16 };
            let down = |i: usize| -> bool { matches!(f.get(i), Some(&"down") | Some(&"press")) };

            if f[0] == "window" {
                target = f.get(1).map(|s| s.to_string());
                println!("INPUT window={:?}", target);
                continue;
            }
            let Some(win) = target.clone() else {
                println!("INPUT err=no-window cmd={:?}", f[0]);
                continue;
            };

            let ok = match f[0] {
                "motion" => router.send_input(
                    &win,
                    InputEvent::MotionNotify {
                        x: num(1) as i16,
                        y: num(2) as i16,
                        state: mask(3),
                    },
                ),
                "button" => {
                    let (button, x, y, st) = (num(1) as u8, num(3) as i16, num(4) as i16, mask(5));
                    let event = if down(2) {
                        InputEvent::ButtonPress {
                            button,
                            x,
                            y,
                            state: st,
                        }
                    } else {
                        InputEvent::ButtonRelease {
                            button,
                            x,
                            y,
                            state: st,
                        }
                    };
                    router.send_input(&win, event)
                }
                "key" => {
                    let (keycode, st) = (num(1) as u32, mask(3));
                    let event = if down(2) {
                        InputEvent::KeyPress { keycode, state: st }
                    } else {
                        InputEvent::KeyRelease { keycode, state: st }
                    };
                    router.send_input(&win, event)
                }
                "manage" => {
                    let action = match f.get(1) {
                        Some(&"close") => WindowWmState::Close,
                        Some(&"minimize") => WindowWmState::Minimized,
                        Some(&"maximize") => WindowWmState::Maximized,
                        Some(&"fullscreen") => WindowWmState::Fullscreen,
                        _ => WindowWmState::Normal,
                    };
                    router.send_input(&win, InputEvent::WindowManage { action })
                }
                "resize" => router.send_resize(&win, num(1) as u16, num(2) as u16),
                other => {
                    println!("INPUT err=unknown-command cmd={other}");
                    continue;
                }
            };
            println!("INPUT cmd={} routed={ok}", f[0]);
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Decimal, or hex with a `0x` prefix — masks are far more readable
/// written the way the frontend's constants are.
#[cfg(target_os = "linux")]
fn parse_int(s: &str) -> Option<i64> {
    match s.strip_prefix("0x") {
        Some(hex) => i64::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}
