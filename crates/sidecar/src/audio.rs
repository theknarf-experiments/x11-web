//! PulseAudio integration.
//!
//! Currently just spawns the per-sidecar pulseaudio daemon so X11
//! apps that talk to libpulse have something to connect to. The
//! capture / playback bridges that fed audio into WebRTC went away
//! with the WebSocket+WebRTC architecture; they will come back over
//! the new QUIC wire when we wire audio into the protocol.

use std::process::Stdio;

use tokio::process::{Child, Command};
use tracing::{info, warn};

/// Start PulseAudio daemon in the background. Returns the child process handle
/// to keep it alive.
pub async fn start_pulseaudio() -> Option<Child> {
    let child = match Command::new("pulseaudio")
        .args([
            "--system",
            "--disallow-exit",
            "--exit-idle-time=-1",
            "--log-level=error",
            "--use-pid-file=false",
            "--daemonize",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("PulseAudio not started ({e}); audio will be unavailable");
            return None;
        }
    };

    // Give PA a moment to start.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Check it's running.
    match Command::new("pactl").arg("info").output().await {
        Ok(output) if output.status.success() => {
            info!("PulseAudio daemon running");
        }
        _ => {
            warn!("PulseAudio may not be running correctly");
        }
    }

    Some(child)
}
