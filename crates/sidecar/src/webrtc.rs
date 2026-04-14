//! WebRTC peer connection manager using str0m (Sans-IO).
//!
//! Each frontend gets its own `Rtc` instance. The sidecar creates an SDP
//! offer (with a data channel for display/input and optionally an audio
//! track), sends it via the existing WebSocket signaling path, and then
//! drives the str0m state machine from a tokio task.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use str0m::change::{SdpAnswer, SdpPendingOffer};
use str0m::channel::{ChannelData, ChannelId};
use str0m::media::{Direction, Frequency, MediaKind, MediaTime, Mid};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, Input, Output, Rtc};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use x11_web_protocol::*;

/// Commands sent to the WebRTC manager task.
#[derive(Debug)]
pub enum RtcCommand {
    /// A frontend wants to establish a WebRTC connection.
    Connect { frontend_id: String },
    /// SDP answer from a frontend.
    Answer { frontend_id: String, sdp: String },
    /// ICE candidate from a frontend.
    IceCandidate {
        frontend_id: String,
        candidate: String,
    },
    /// Display update to send to all connected peers via data channel.
    DisplayUpdate {
        client_id: String,
        update: DisplayUpdate,
    },
    /// Clipboard data to send via data channel.
    ClipboardData {
        selection: String,
        mime_type: String,
        data: Vec<u8>,
    },
    /// Clipboard offer to send via data channel.
    ClipboardOffer {
        selection: String,
        mime_types: Vec<String>,
    },
    /// Process connected event.
    ProcessConnected {
        pid: u32,
        client_id: String,
        command: String,
    },
    /// Process exited event.
    ProcessExited {
        pid: u32,
        exit_code: Option<i32>,
    },
    /// Input was dropped.
    InputDropped {
        window_id: String,
        reason: String,
    },
    /// Send audio data (Opus encoded) to all peers.
    AudioData {
        data: Vec<u8>,
        duration_ms: u32,
    },
    /// UDP packet received from a peer's socket (used once UDP receive loop is wired up).
    #[allow(dead_code)]
    UdpInput {
        local_addr: SocketAddr,
        source: SocketAddr,
        data: Vec<u8>,
    },
}

/// Events produced by the WebRTC manager for the main sidecar loop.
#[derive(Debug)]
pub enum RtcEvent {
    /// Signaling message to send via WebSocket to backend.
    Signal(SidecarToBackend),
    /// Input event received from a frontend via data channel.
    Input {
        window_id: String,
        event: InputEvent,
    },
    /// Redraw request from frontend.
    Redraw { window_id: String },
    /// Window resize from frontend.
    ResizeWindow {
        window_id: String,
        width: u16,
        height: u16,
    },
    /// Screen resize from frontend.
    ResizeScreen { width: u16, height: u16 },
    /// Clipboard set from frontend.
    SetClipboard {
        selection: String,
        mime_type: String,
        data: Vec<u8>,
    },
    /// Clipboard request from frontend.
    RequestClipboard {
        selection: String,
        mime_type: String,
    },
    /// Spawn process request from frontend.
    SpawnProcess {
        request_id: String,
        command: String,
        args: Vec<String>,
    },
    /// Kill process request from frontend.
    KillProcess { request_id: String, pid: u32 },
    /// Audio data received from a frontend (microphone).
    MicAudio { data: Vec<u8> },
}

/// State for a single peer connection (one per frontend).
struct PeerConnection {
    rtc: Rtc,
    socket: Arc<UdpSocket>,
    local_addr: SocketAddr,
    /// Pending offer waiting for answer from the remote peer.
    pending_offer: Option<SdpPendingOffer>,
    data_channel_id: Option<ChannelId>,
    audio_mid: Option<Mid>,
    audio_samples: u64,
}

/// The WebRTC manager: owns all peer connections, driven by a tokio task.
pub struct WebRtcManager {
    cmd_tx: mpsc::UnboundedSender<RtcCommand>,
}

impl WebRtcManager {
    /// Spawn the WebRTC manager task. Returns the manager handle and the
    /// event receiver for the main loop.
    pub fn spawn() -> (Self, mpsc::UnboundedReceiver<RtcEvent>) {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (evt_tx, evt_rx) = mpsc::unbounded_channel();

        tokio::spawn(run_manager(cmd_rx, evt_tx));

        (Self { cmd_tx }, evt_rx)
    }

    pub fn send(&self, cmd: RtcCommand) {
        if self.cmd_tx.send(cmd).is_err() {
            warn!("WebRTC manager task has exited");
        }
    }

    /// Clone the command sender for use by the audio capture task.
    pub fn cmd_tx_clone(&self) -> mpsc::UnboundedSender<RtcCommand> {
        self.cmd_tx.clone()
    }
}

/// Bind a UDP socket on an available port.
async fn bind_udp() -> std::io::Result<(UdpSocket, SocketAddr)> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let local_addr = socket.local_addr()?;
    Ok((socket, local_addr))
}

/// Main manager loop. Each frontend gets its own PeerConnection.
async fn run_manager(
    mut cmd_rx: mpsc::UnboundedReceiver<RtcCommand>,
    evt_tx: mpsc::UnboundedSender<RtcEvent>,
) {
    let mut peers: HashMap<String, PeerConnection> = HashMap::new();
    // Map from socket local addr to frontend_id for UDP demux.
    let mut addr_to_frontend: HashMap<SocketAddr, String> = HashMap::new();

    info!("WebRTC manager started");

    loop {
        // Drain all pending outputs from all peers first.
        let frontend_ids: Vec<String> = peers.keys().cloned().collect();
        for fid in &frontend_ids {
            if let Some(peer) = peers.get_mut(fid) {
                process_peer_outputs(peer, fid, &evt_tx).await;
            }
        }

        // Find the earliest timeout across all peers.
        let next_timeout = peers
            .values_mut()
            .filter_map(|p| match p.rtc.poll_output() {
                Ok(Output::Timeout(t)) => Some(t),
                _ => None,
            })
            .min();

        let sleep_dur = next_timeout
            .map(|t| {
                t.checked_duration_since(Instant::now())
                    .unwrap_or(Duration::ZERO)
            })
            .unwrap_or(Duration::from_millis(100));

        tokio::select! {
            Some(cmd) = cmd_rx.recv() => {
                handle_command(cmd, &mut peers, &mut addr_to_frontend, &evt_tx).await;
            }
            _ = tokio::time::sleep(sleep_dur) => {
                // Drive timeouts for all peers.
                let now = Instant::now();
                for peer in peers.values_mut() {
                    let _ = peer.rtc.handle_input(Input::Timeout(now));
                }
            }
        }

        // Remove disconnected peers.
        peers.retain(|fid, peer| {
            if peer.rtc.is_alive() {
                true
            } else {
                info!("WebRTC peer {fid} disconnected, cleaning up");
                addr_to_frontend.remove(&peer.local_addr);
                false
            }
        });
    }
}

/// Send binary data on the data channel of a peer if it's open.
fn dc_send(peer: &mut PeerConnection, data: &[u8]) {
    if let Some(ch_id) = peer.data_channel_id {
        if let Some(mut ch) = peer.rtc.channel(ch_id) {
            let _ = ch.write(true, data);
        }
    }
}

/// Broadcast a DcServerMsg to all peers with open data channels.
fn broadcast_dc(peers: &mut HashMap<String, PeerConnection>, msg: &DcServerMsg) {
    if let Ok(encoded) = dc_encode(msg) {
        for peer in peers.values_mut() {
            dc_send(peer, &encoded);
        }
    }
}

async fn handle_command(
    cmd: RtcCommand,
    peers: &mut HashMap<String, PeerConnection>,
    addr_to_frontend: &mut HashMap<SocketAddr, String>,
    evt_tx: &mpsc::UnboundedSender<RtcEvent>,
) {
    match cmd {
        RtcCommand::Connect { frontend_id } => {
            info!("Creating WebRTC offer for frontend {frontend_id}");
            match create_peer_connection(&frontend_id).await {
                Ok((peer, offer_sdp)) => {
                    addr_to_frontend.insert(peer.local_addr, frontend_id.clone());

                    // Send the offer via signaling.
                    let _ = evt_tx.send(RtcEvent::Signal(SidecarToBackend::RtcOffer {
                        frontend_id: frontend_id.clone(),
                        sdp: offer_sdp,
                    }));

                    peers.insert(frontend_id, peer);
                }
                Err(e) => {
                    error!("Failed to create peer connection: {e}");
                }
            }
        }
        RtcCommand::Answer { frontend_id, sdp } => {
            if sdp.is_empty() {
                // This is a "connect" request (empty SDP = please create offer).
                info!("RtcConnect request from frontend {frontend_id}");
                Box::pin(handle_command(
                    RtcCommand::Connect { frontend_id },
                    peers,
                    addr_to_frontend,
                    evt_tx,
                ))
                .await;
                return;
            }

            if let Some(peer) = peers.get_mut(&frontend_id) {
                let answer = match SdpAnswer::from_sdp_string(&sdp) {
                    Ok(a) => a,
                    Err(e) => {
                        error!("Failed to parse SDP answer from {frontend_id}: {e}");
                        return;
                    }
                };
                if let Some(pending) = peer.pending_offer.take() {
                    info!("Applying SDP answer from frontend {frontend_id}");
                    match peer.rtc.sdp_api().accept_answer(pending, answer) {
                        Ok(()) => {
                            info!("SDP answer applied for {frontend_id}");
                        }
                        Err(e) => {
                            error!("Failed to apply SDP answer for {frontend_id}: {e}");
                        }
                    }
                } else {
                    warn!("No pending offer for frontend {frontend_id}");
                }
            } else {
                warn!("No peer connection for frontend {frontend_id}");
            }
        }
        RtcCommand::IceCandidate {
            frontend_id,
            candidate,
        } => {
            if let Some(peer) = peers.get_mut(&frontend_id) {
                // Parse a=candidate line. str0m's Candidate type can be built
                // from the candidate attribute string.
                match Candidate::from_sdp_string(&candidate) {
                    Ok(c) => {
                        peer.rtc.add_remote_candidate(c);
                        debug!("Added remote ICE candidate for {frontend_id}");
                    }
                    Err(e) => {
                        warn!("Failed to parse ICE candidate for {frontend_id}: {e}");
                    }
                }
            }
        }
        RtcCommand::UdpInput {
            local_addr,
            source,
            data,
        } => {
            // Find the peer whose socket matches this local address.
            if let Some(fid) = addr_to_frontend.get(&local_addr).cloned() {
                if let Some(peer) = peers.get_mut(&fid) {
                    let now = Instant::now();
                    match Receive::new(Protocol::Udp, source, local_addr, &data) {
                        Ok(receive) => {
                            let _ = peer.rtc.handle_input(Input::Receive(now, receive));
                        }
                        Err(e) => {
                            debug!("Failed to parse UDP datagram: {e}");
                        }
                    }
                }
            }
        }
        RtcCommand::DisplayUpdate { client_id, update } => {
            broadcast_dc(
                peers,
                &DcServerMsg::Display { client_id, update },
            );
        }
        RtcCommand::ClipboardData {
            selection,
            mime_type,
            data,
        } => {
            broadcast_dc(
                peers,
                &DcServerMsg::Clipboard {
                    selection,
                    mime_type,
                    data,
                },
            );
        }
        RtcCommand::ClipboardOffer {
            selection,
            mime_types,
        } => {
            broadcast_dc(
                peers,
                &DcServerMsg::ClipboardOffer {
                    selection,
                    mime_types,
                },
            );
        }
        RtcCommand::ProcessConnected {
            pid,
            client_id,
            command,
        } => {
            broadcast_dc(
                peers,
                &DcServerMsg::ProcessConnected {
                    pid,
                    client_id,
                    command,
                },
            );
        }
        RtcCommand::ProcessExited { pid, exit_code } => {
            broadcast_dc(peers, &DcServerMsg::ProcessExited { pid, exit_code });
        }
        RtcCommand::InputDropped { window_id, reason } => {
            broadcast_dc(
                peers,
                &DcServerMsg::InputDropped { window_id, reason },
            );
        }
        RtcCommand::AudioData { data, duration_ms } => {
            let now = Instant::now();
            for peer in peers.values_mut() {
                if let Some(mid) = peer.audio_mid {
                    // Get the payload type first, then write in a separate borrow.
                    let pt = peer
                        .rtc
                        .writer(mid)
                        .and_then(|w| w.payload_params().next().map(|p| p.pt()));
                    if let Some(pt) = pt {
                        let samples = duration_ms as u64 * 48; // Opus at 48kHz
                        peer.audio_samples += samples;
                        let rtp_time =
                            MediaTime::new(peer.audio_samples, Frequency::FORTY_EIGHT_KHZ);
                        if let Some(writer) = peer.rtc.writer(mid) {
                            let _ = writer.write(pt, now, rtp_time, data.clone());
                        }
                    }
                }
            }
        }
    }
}

/// Create a new peer connection with data channel and audio track.
async fn create_peer_connection(
    frontend_id: &str,
) -> Result<(PeerConnection, String), Box<dyn std::error::Error + Send + Sync>> {
    let (socket, local_addr) = bind_udp().await?;
    info!("Bound UDP for WebRTC peer {frontend_id} at {local_addr}");

    let mut rtc = Rtc::builder().set_ice_lite(true).build(Instant::now());

    // Add local ICE candidate (the UDP address we're listening on).
    let candidate = Candidate::host(local_addr, "udp")?;
    rtc.add_local_candidate(candidate);

    // Build SDP offer: audio track + data channel.
    let mut change = rtc.sdp_api();
    let audio_mid = change.add_media(MediaKind::Audio, Direction::SendRecv, None, None, None);
    let _channel_id = change.add_channel("x11data".to_string());

    let (offer, pending) = change
        .apply()
        .ok_or("SDP apply returned None (no changes to negotiate)")?;
    let offer_sdp = offer.to_sdp_string();

    let peer = PeerConnection {
        rtc,
        socket: Arc::new(socket),
        local_addr,
        pending_offer: Some(pending),
        data_channel_id: None,
        audio_mid: Some(audio_mid),
        audio_samples: 0,
    };

    Ok((peer, offer_sdp))
}

/// Process all pending outputs from a peer's Rtc instance.
async fn process_peer_outputs(
    peer: &mut PeerConnection,
    frontend_id: &str,
    evt_tx: &mpsc::UnboundedSender<RtcEvent>,
) {
    loop {
        match peer.rtc.poll_output() {
            Ok(Output::Timeout(_)) => break,
            Ok(Output::Transmit(t)) => {
                let socket = peer.socket.clone();
                let data = t.contents.to_vec();
                let dest = t.destination;
                tokio::spawn(async move {
                    let _ = socket.send_to(&data, dest).await;
                });
            }
            Ok(Output::Event(event)) => {
                handle_peer_event(peer, frontend_id, event, evt_tx);
            }
            Err(e) => {
                warn!("str0m error for peer {frontend_id}: {e}");
                break;
            }
        }
    }
}

/// Handle a single str0m event.
fn handle_peer_event(
    peer: &mut PeerConnection,
    frontend_id: &str,
    event: Event,
    evt_tx: &mpsc::UnboundedSender<RtcEvent>,
) {
    match event {
        Event::IceConnectionStateChange(state) => {
            info!("WebRTC peer {frontend_id} ICE state: {state:?}");
        }
        Event::Connected => {
            info!("WebRTC peer {frontend_id} connected");
        }
        Event::ChannelOpen(ch_id, label) => {
            info!("Data channel opened for {frontend_id}: {label} (id: {ch_id:?})");
            peer.data_channel_id = Some(ch_id);
        }
        Event::ChannelData(data) => {
            handle_channel_data(frontend_id, data, evt_tx);
        }
        Event::ChannelClose(ch_id) => {
            info!("Data channel closed for {frontend_id}: {ch_id:?}");
            if peer.data_channel_id == Some(ch_id) {
                peer.data_channel_id = None;
            }
        }
        Event::MediaAdded(event) => {
            info!(
                "Media added for {frontend_id}: mid={:?} kind={:?} dir={:?}",
                event.mid, event.kind, event.direction
            );
        }
        Event::MediaData(media_data) => {
            // Incoming audio from browser (microphone).
            let _ = evt_tx.send(RtcEvent::MicAudio {
                data: media_data.data,
            });
        }
        _ => {
            debug!("str0m event for {frontend_id}: {event:?}");
        }
    }
}

/// Handle incoming data channel message from frontend.
fn handle_channel_data(
    frontend_id: &str,
    data: ChannelData,
    evt_tx: &mpsc::UnboundedSender<RtcEvent>,
) {
    let msg: DcClientMsg = match dc_decode(&data.data) {
        Ok(m) => m,
        Err(e) => {
            warn!("Bad data channel msg from {frontend_id}: {e}");
            return;
        }
    };

    let event = match msg {
        DcClientMsg::Input { window_id, event } => RtcEvent::Input { window_id, event },
        DcClientMsg::Redraw { window_id } => RtcEvent::Redraw { window_id },
        DcClientMsg::ResizeWindow {
            window_id,
            width,
            height,
        } => RtcEvent::ResizeWindow {
            window_id,
            width,
            height,
        },
        DcClientMsg::ResizeScreen { width, height } => RtcEvent::ResizeScreen { width, height },
        DcClientMsg::SetClipboard {
            selection,
            mime_type,
            data,
        } => RtcEvent::SetClipboard {
            selection,
            mime_type,
            data,
        },
        DcClientMsg::RequestClipboard {
            selection,
            mime_type,
        } => RtcEvent::RequestClipboard {
            selection,
            mime_type,
        },
        DcClientMsg::SpawnProcess {
            request_id,
            command,
            args,
        } => RtcEvent::SpawnProcess {
            request_id,
            command,
            args,
        },
        DcClientMsg::KillProcess { request_id, pid } => {
            RtcEvent::KillProcess { request_id, pid }
        }
    };
    let _ = evt_tx.send(event);
}
