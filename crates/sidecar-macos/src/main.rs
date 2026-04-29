#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("x11-web-sidecar-macos only builds on macOS.");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() {
    macos::run().await;
}

#[cfg(target_os = "macos")]
mod macos {
    use std::time::Duration;

    use futures::{SinkExt, StreamExt};
    use tokio::sync::mpsc;
    use tokio::time::interval;
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message;
    use tracing::{error, info, warn};
    use x11_web_protocol::SidecarToBackend;

    pub async fn run() {
        tracing_subscriber::fmt::init();

        // Probe SkyLight up front so the operator sees in the log
        // whether the private path is reachable on this system.
        let sky = x11_web_sidecar_macos::skylight::probe();
        info!(
            "SkyLight bridge: post_to_pid={} auth_message={} window_location={}",
            sky.post_to_pid, sky.auth_message, sky.window_location
        );

        let backend_url = std::env::var("BACKEND_URL")
            .unwrap_or_else(|_| "ws://127.0.0.1:3001/ws/sidecar".into());
        let sidecar_name = std::env::var("SIDECAR_NAME")
            .unwrap_or_else(|_| hostname().unwrap_or_else(|| "macos-sidecar".into()));

        info!("Connecting to backend at {backend_url}");
        loop {
            match connect_async(&backend_url).await {
                Ok((ws_stream, _)) => {
                    info!("Connected to backend");
                    run_session(ws_stream, &sidecar_name).await;
                    warn!("Disconnected from backend, reconnecting in 5s...");
                }
                Err(e) => {
                    error!("Failed to connect to backend: {e}");
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    async fn run_session(
        ws_stream: tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        sidecar_name: &str,
    ) {
        let (mut ws_tx, mut ws_rx) = ws_stream.split();

        let register = SidecarToBackend::Register {
            sidecar_name: sidecar_name.to_string(),
        };
        let json = serde_json::to_string(&register).unwrap();
        if ws_tx.send(Message::Text(json.into())).await.is_err() {
            return;
        }

        let (tx, mut rx) = mpsc::unbounded_channel::<SidecarToBackend>();

        let tx_heartbeat = tx.clone();
        let heartbeat_task = tokio::spawn(async move {
            let mut tick = interval(Duration::from_secs(30));
            loop {
                tick.tick().await;
                if tx_heartbeat.send(SidecarToBackend::Heartbeat).is_err() {
                    break;
                }
            }
        });

        let send_task = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let json = serde_json::to_string(&msg).unwrap();
                if ws_tx.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        });

        // Window enumeration → DisplayUpdate stream. Per-session so the
        // backend gets a fresh "everything that exists" announcement
        // each time we reconnect.
        x11_web_sidecar_macos::enumerator::spawn(tx.clone());

        // v0.1 minimum: keep the connection alive while the enumerator
        // streams updates. Capture and input come in subsequent commits.
        loop {
            match ws_rx.next().await {
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(e)) => {
                    warn!("WebSocket recv error: {e}");
                    break;
                }
            }
        }

        heartbeat_task.abort();
        send_task.abort();
    }

    fn hostname() -> Option<String> {
        std::env::var("HOSTNAME").ok().or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
    }
}
