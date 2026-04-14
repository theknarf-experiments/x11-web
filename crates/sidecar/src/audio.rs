//! PulseAudio integration for audio capture and playback.
//!
//! - Captures audio from the PulseAudio monitor source (apps' audio output).
//! - Encodes to Opus and sends via WebRTC audio track.
//! - Receives decoded audio from WebRTC (browser mic) and pipes to PulseAudio virtual source.

use std::process::Stdio;
use std::sync::Arc;

use tokio::io::AsyncBufReadExt;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::webrtc::RtcCommand;

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

/// Audio capture task: reads PCM from PulseAudio monitor, encodes to Opus,
/// and sends encoded frames to the WebRTC manager.
///
/// Runs until the sender is dropped or PulseAudio goes away.
pub async fn run_audio_capture(rtc_tx: mpsc::UnboundedSender<RtcCommand>) {
    // Use parec (PulseAudio recording utility) to capture PCM from the monitor.
    let mut parec = match Command::new("parec")
        .args([
            "--format=s16le",
            "--rate=48000",
            "--channels=1",
            "--device=virtual_out.monitor",
            "--latency-msec=20",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("parec not available ({e}); audio capture disabled");
            return;
        }
    };

    let stdout = match parec.stdout.take() {
        Some(s) => s,
        None => {
            warn!("parec stdout unavailable");
            return;
        }
    };

    info!("Audio capture started (48kHz mono s16le)");

    // Initialize Opus encoder.
    let mut encoder = match opus::Encoder::new(48000, opus::Channels::Mono, opus::Application::Audio) {
        Ok(e) => e,
        Err(e) => {
            error!("Failed to create Opus encoder: {e}");
            return;
        }
    };

    // Read 20ms frames (960 samples at 48kHz mono = 1920 bytes of s16le).
    const FRAME_SIZE: usize = 960; // samples per 20ms at 48kHz
    const BYTES_PER_FRAME: usize = FRAME_SIZE * 2; // s16le = 2 bytes/sample
    let mut pcm_buf = vec![0u8; BYTES_PER_FRAME];
    let mut opus_buf = vec![0u8; 4000]; // max Opus frame size
    let mut reader = tokio::io::BufReader::new(stdout);

    loop {
        // Read exactly one frame of PCM data.
        match tokio::io::AsyncReadExt::read_exact(&mut reader, &mut pcm_buf).await {
            Ok(_) => {}
            Err(e) => {
                if e.kind() != std::io::ErrorKind::UnexpectedEof {
                    warn!("Audio capture read error: {e}");
                }
                break;
            }
        }

        // Convert bytes to i16 samples.
        let samples: Vec<i16> = pcm_buf
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();

        // Encode to Opus.
        match encoder.encode(&samples, &mut opus_buf) {
            Ok(len) => {
                let opus_frame = opus_buf[..len].to_vec();
                if rtc_tx
                    .send(RtcCommand::AudioData {
                        data: opus_frame,
                        duration_ms: 20,
                    })
                    .is_err()
                {
                    break; // WebRTC manager gone.
                }
            }
            Err(e) => {
                warn!("Opus encode error: {e}");
            }
        }
    }

    info!("Audio capture stopped");
    let _ = parec.kill().await;
}

/// Write decoded audio (PCM s16le 48kHz mono) to PulseAudio's virtual input
/// so it appears as a microphone source for X11 apps.
pub async fn run_audio_playback(mut rx: mpsc::UnboundedReceiver<Vec<u8>>) {
    // Use pacat to play PCM into the virtual input sink.
    let mut pacat = match Command::new("pacat")
        .args([
            "--format=s16le",
            "--rate=48000",
            "--channels=1",
            "--device=virtual_in",
            "--latency-msec=20",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("pacat not available ({e}); mic playback disabled");
            return;
        }
    };

    let mut stdin = match pacat.stdin.take() {
        Some(s) => s,
        None => {
            warn!("pacat stdin unavailable");
            return;
        }
    };

    info!("Mic playback started (virtual input sink)");

    // Opus decoder for incoming audio.
    let mut decoder = match opus::Decoder::new(48000, opus::Channels::Mono) {
        Ok(d) => d,
        Err(e) => {
            error!("Failed to create Opus decoder: {e}");
            return;
        }
    };
    let mut pcm_buf = vec![0i16; 960]; // 20ms at 48kHz

    while let Some(opus_data) = rx.recv().await {
        match decoder.decode(&opus_data, &mut pcm_buf, false) {
            Ok(samples) => {
                let bytes: Vec<u8> = pcm_buf[..samples]
                    .iter()
                    .flat_map(|s| s.to_le_bytes())
                    .collect();
                if tokio::io::AsyncWriteExt::write_all(&mut stdin, &bytes)
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Err(e) => {
                warn!("Opus decode error: {e}");
            }
        }
    }

    info!("Mic playback stopped");
    let _ = pacat.kill().await;
}
