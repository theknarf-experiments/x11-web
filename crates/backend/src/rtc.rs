//! Per-frontend WebRTC peer + DataChannel driver.
//!
//! str0m is sans-IO: we own the UDP socket and feed bytes / events
//! through `Rtc::handle_input` and `Rtc::poll_output`. Each frontend
//! gets its own task with its own UDP socket and `Rtc`. SDP exchange
//! and ICE trickling ride the existing WebSocket signalling path —
//! see `RtcOffer` / `RtcAnswer` / `RtcIceCandidate` on the protocol
//! crate's frontend-facing enums.
//!
//! Host candidates only for now; STUN/TURN can be layered on later
//! when we deploy beyond LAN.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use str0m::change::SdpOffer;
use str0m::channel::ChannelId;
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, Input, Output, Rtc};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Notify};
use tracing::{debug, info, warn};
use x11_web_protocol::BackendToFrontend;

use crate::chunking::Chunker;

/// Handle the WS task uses to drive the per-frontend Rtc.
pub struct RtcConn {
    /// SDP offers + remote ICE candidates received from the browser
    /// over the WebSocket; forwarded into `Rtc`.
    pub signal_tx: mpsc::UnboundedSender<RtcSignal>,
    /// Bytes to write into the media DataChannel ("putimage", unordered+
    /// unreliable). Dropped silently if the channel isn't open yet —
    /// callers should gate on `dc_open`.
    pub dc_tx: mpsc::UnboundedSender<Vec<u8>>,
    /// True iff the media DC has been opened by the peer at least once
    /// and hasn't closed since.
    pub dc_open: Arc<AtomicBool>,
    /// Pulsed once on the media DC's first open. Replay tasks await
    /// this to know when it's safe to start sending bytes via `dc_tx`.
    pub dc_opened: Arc<Notify>,
    /// Bytes to write into the control DataChannel ("control",
    /// ordered+reliable). Used for Automerge sync and any other
    /// loss-intolerant traffic. Same caveat as `dc_tx` — gate on
    /// `control_open`.
    pub control_tx: mpsc::UnboundedSender<Vec<u8>>,
    #[allow(dead_code)] // read in slice 1b (workspace-sync gating)
    pub control_open: Arc<AtomicBool>,
    pub control_opened: Arc<Notify>,
}

#[derive(Debug)]
pub enum RtcSignal {
    Offer(String),
    /// Trickled ICE candidate from the browser. `sdpMid` /
    /// `sdpMLineIndex` from the WS message are dropped here because
    /// `str0m::Candidate::from_sdp_string` parses everything it needs
    /// from the candidate line itself.
    IceCandidate(String),
}

/// Spawns the driver task and returns a handle. The task lives until
/// any of its channels close. `control_inbound_tx` receives raw bytes
/// from the control DC — caller is responsible for decoding and
/// dispatching (e.g., to a workspace-sync handler).
pub fn spawn(
    frontend_id: String,
    ws_tx: mpsc::UnboundedSender<BackendToFrontend>,
    control_inbound_tx: mpsc::UnboundedSender<Vec<u8>>,
) -> RtcConn {
    let (signal_tx, signal_rx) = mpsc::unbounded_channel::<RtcSignal>();
    let (dc_tx, dc_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (control_tx, control_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let dc_open = Arc::new(AtomicBool::new(false));
    let dc_opened = Arc::new(Notify::new());
    let control_open = Arc::new(AtomicBool::new(false));
    let control_opened = Arc::new(Notify::new());
    let dc_open_for_task = dc_open.clone();
    let dc_opened_for_task = dc_opened.clone();
    let control_open_for_task = control_open.clone();
    let control_opened_for_task = control_opened.clone();

    tokio::spawn(async move {
        if let Err(e) = drive(
            frontend_id,
            ws_tx,
            signal_rx,
            dc_rx,
            control_rx,
            control_inbound_tx,
            dc_open_for_task,
            dc_opened_for_task,
            control_open_for_task,
            control_opened_for_task,
        )
        .await
        {
            warn!("rtc task ended with error: {e}");
        }
    });

    RtcConn {
        signal_tx,
        dc_tx,
        dc_open,
        dc_opened,
        control_tx,
        control_open,
        control_opened,
    }
}

#[allow(clippy::too_many_arguments)]
async fn drive(
    frontend_id: String,
    ws_tx: mpsc::UnboundedSender<BackendToFrontend>,
    mut signal_rx: mpsc::UnboundedReceiver<RtcSignal>,
    mut dc_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    mut control_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    control_inbound_tx: mpsc::UnboundedSender<Vec<u8>>,
    dc_open: Arc<AtomicBool>,
    dc_opened: Arc<Notify>,
    control_open: Arc<AtomicBool>,
    control_opened: Arc<Notify>,
) -> Result<(), String> {
    // Bind / public address are env-configurable so the same binary
    // works on the host (mprocs / dev) and inside Docker (e2e):
    //
    //   X11WEB_RTC_BIND_ADDR — what to UDP-bind to
    //                          (default `127.0.0.1:0`).
    //   X11WEB_RTC_PUBLIC_HOST — IP to advertise as the host
    //                            candidate (default `127.0.0.1`).
    //
    // Critical detail: str0m's `Receive::new` takes the DESTINATION
    // address the packet was sent to. If we bound `0.0.0.0:N` and
    // reported `0.0.0.0:N` as the destination, ICE candidate matching
    // would fail. Binding to a specific address means we can hand
    // str0m the exact same address it has in its candidate list.
    let bind_addr_str =
        std::env::var("X11WEB_RTC_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:0".to_string());
    let socket = UdpSocket::bind(&bind_addr_str)
        .await
        .map_err(|e| format!("bind UDP {bind_addr_str}: {e}"))?;
    let local_addr = socket
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?;
    let public_host: std::net::IpAddr = std::env::var("X11WEB_RTC_PUBLIC_HOST")
        .unwrap_or_else(|_| "127.0.0.1".to_string())
        .parse()
        .map_err(|e| format!("invalid X11WEB_RTC_PUBLIC_HOST: {e}"))?;
    let public_addr = std::net::SocketAddr::new(public_host, local_addr.port());
    info!(%frontend_id, %local_addr, %public_addr, "rtc UDP socket bound");

    // Block until the offer arrives, then we have everything to build
    // the Rtc and reply with the answer.
    let mut rtc = loop {
        match signal_rx.recv().await {
            Some(RtcSignal::Offer(sdp)) => {
                let offer =
                    SdpOffer::from_sdp_string(&sdp).map_err(|e| format!("parse offer: {e}"))?;
                let mut rtc = Rtc::builder().build(Instant::now());
                let candidate = Candidate::host(public_addr, "udp")
                    .map_err(|e| format!("host candidate: {e}"))?;
                rtc.add_local_candidate(candidate);
                let answer = rtc
                    .sdp_api()
                    .accept_offer(offer)
                    .map_err(|e| format!("accept_offer: {e}"))?;
                let _ = ws_tx.send(BackendToFrontend::RtcAnswer {
                    sdp: answer.to_sdp_string(),
                });
                break rtc;
            }
            Some(other) => {
                debug!(?other, "rtc signal before offer — ignoring");
            }
            None => return Ok(()),
        }
    };

    // Two channels, dispatched by label. The browser-side
    // `pc.createDataChannel` calls determine the labels; we just
    // route inbound/outbound by what we see on `ChannelOpen`.
    let mut media_channel_id: Option<ChannelId> = None;
    let mut control_channel_id: Option<ChannelId> = None;
    let mut buf = vec![0u8; 2000];
    let mut chunker = Chunker::new();
    // Backpressure queue. New PutImage frames REPLACE the queue
    // (latest-frame-wins, the right semantic for unordered+unreliable
    // media — old chunks would just produce a stale frame anyway).
    let mut pending: VecDeque<Vec<u8>> = VecDeque::new();
    // Threshold below which str0m emits ChannelBufferedAmountLow,
    // signalling there's room for more writes.
    const LOW_THRESHOLD: usize = 32 * 1024;

    loop {
        // Drain pending chunks while str0m has buffer room. We do this
        // at the top of every loop iteration AND on
        // `ChannelBufferedAmountLow` so writes flow continuously.
        drain_pending(&mut rtc, media_channel_id, &mut pending, &frontend_id);

        // Drain non-blocking output before parking on input. Each
        // iteration of this inner loop processes one Output; when we
        // hit Timeout we fall through to the select! below.
        let timeout_at = loop {
            match rtc.poll_output().map_err(|e| format!("poll_output: {e}"))? {
                Output::Timeout(t) => break t,
                Output::Transmit(t) => {
                    if let Err(e) = socket.send_to(&t.contents, t.destination).await {
                        warn!(%frontend_id, "udp send: {e}");
                    }
                }
                Output::Event(event) => match event {
                    Event::ChannelOpen(id, label) => {
                        info!(%frontend_id, %label, "DC open");
                        match label.as_str() {
                            "control" => {
                                control_channel_id = Some(id);
                                control_open.store(true, Ordering::Release);
                                control_opened.notify_waiters();
                            }
                            // Default to media for any other label
                            // (currently "putimage"). Keeps backward
                            // compatibility if the browser-side label
                            // ever changes.
                            _ => {
                                media_channel_id = Some(id);
                                if let Some(mut ch) = rtc.channel(id) {
                                    ch.set_buffered_amount_low_threshold(LOW_THRESHOLD);
                                }
                                dc_open.store(true, Ordering::Release);
                                dc_opened.notify_waiters();
                            }
                        }
                    }
                    Event::ChannelClose(id) => {
                        if Some(id) == media_channel_id {
                            info!(%frontend_id, "media DC close");
                            media_channel_id = None;
                            pending.clear();
                            dc_open.store(false, Ordering::Release);
                        } else if Some(id) == control_channel_id {
                            info!(%frontend_id, "control DC close");
                            control_channel_id = None;
                            control_open.store(false, Ordering::Release);
                        }
                    }
                    Event::ChannelData(d) => {
                        if Some(d.id) == control_channel_id {
                            // Control channel is loss-intolerant —
                            // forward verbatim to whoever owns the
                            // inbound rx. Data is the raw capnp Frame
                            // bytes; decoding lives upstream.
                            if control_inbound_tx.send(d.data).is_err() {
                                debug!(%frontend_id, "control inbound rx dropped");
                            }
                        } else {
                            debug!(%frontend_id, "media DC inbound ({} bytes) — ignoring", d.data.len());
                        }
                    }
                    Event::ChannelBufferedAmountLow(_) => {
                        drain_pending(&mut rtc, media_channel_id, &mut pending, &frontend_id);
                    }
                    Event::IceConnectionStateChange(state) => {
                        debug!(%frontend_id, ?state, "ICE state");
                    }
                    _ => {}
                },
            }
        };

        let now = Instant::now();
        let dur = if timeout_at > now {
            timeout_at - now
        } else {
            Duration::ZERO
        };

        tokio::select! {
            _ = tokio::time::sleep(dur) => {
                rtc.handle_input(Input::Timeout(Instant::now()))
                    .map_err(|e| format!("handle timeout: {e}"))?;
            }
            res = socket.recv_from(&mut buf) => {
                match res {
                    Ok((n, src)) => {
                        // Hand str0m the *public* address as the
                        // destination. With `0.0.0.0` binds we don't
                        // know the actual destination IP from
                        // `recv_from`, but with our specific bind
                        // address (or env-overridden public host) we
                        // can name the same address str0m has in its
                        // candidate list, so its connectivity-check
                        // matching works.
                        let receive = Receive::new(
                            Protocol::Udp,
                            src,
                            public_addr,
                            &buf[..n],
                        ).map_err(|e| format!("receive: {e}"))?;
                        rtc.handle_input(Input::Receive(Instant::now(), receive))
                            .map_err(|e| format!("handle receive: {e}"))?;
                    }
                    Err(e) => {
                        warn!(%frontend_id, "udp recv: {e}");
                    }
                }
            }
            Some(signal) = signal_rx.recv() => {
                if let Err(e) = handle_signal(&mut rtc, signal) {
                    warn!(%frontend_id, "signal: {e}");
                }
            }
            Some(data) = dc_rx.recv() => {
                // Drain any frames queued behind this one in dc_rx
                // without yielding — we only care about the latest.
                // Keeps the rtc task from chunking-then-discarding
                // stale frames when the producer is faster than the
                // network.
                let mut latest = data;
                while let Ok(next) = dc_rx.try_recv() {
                    latest = next;
                }
                if media_channel_id.is_some() {
                    // Latest-frame-wins. If the previous frame's
                    // chunks haven't all drained yet, drop them — they
                    // would only produce a stale image. The chunker's
                    // monotonic msg_id and the frontend reassembler's
                    // stale-msg eviction handle whatever's already on
                    // the wire.
                    pending.clear();
                    for chunk in chunker.chunk(&latest) {
                        pending.push_back(chunk);
                    }
                }
            }
            Some(data) = control_rx.recv() => {
                // Control channel is reliable+ordered. No chunking,
                // no latest-wins — every message must arrive intact.
                // We trust the upstream layer not to ship messages
                // bigger than the SCTP single-message limit (~64KB
                // on most browsers). Automerge sync messages are
                // typically a few hundred bytes to a few KB.
                if let Some(id) = control_channel_id {
                    if let Some(mut ch) = rtc.channel(id) {
                        if let Err(e) = ch.write(true, &data) {
                            warn!(%frontend_id, "control DC write: {e}");
                        }
                    }
                } else {
                    debug!(%frontend_id, "control bytes dropped — DC not open");
                }
            }
            else => break,
        }

        if !rtc.is_alive() {
            info!(%frontend_id, "rtc no longer alive — exiting");
            break;
        }
    }

    Ok(())
}

/// Push as many queued chunks as str0m's SCTP buffer accepts. Stops
/// on the first `Ok(false)` (buffer full) and waits for
/// `ChannelBufferedAmountLow` to retry. Called at the top of every
/// loop iteration plus on the buffered-low event. Operates on the
/// media channel only — the control channel writes are individually
/// addressed and never queued.
fn drain_pending(
    rtc: &mut Rtc,
    media_channel_id: Option<ChannelId>,
    pending: &mut VecDeque<Vec<u8>>,
    frontend_id: &str,
) {
    let Some(id) = media_channel_id else { return };
    while let Some(chunk) = pending.front() {
        let Some(mut ch) = rtc.channel(id) else {
            return;
        };
        match ch.write(true, chunk) {
            Ok(true) => {
                pending.pop_front();
            }
            Ok(false) => return, // buffer full, retry on ChannelBufferedAmountLow
            Err(e) => {
                warn!(%frontend_id, "DC write: {e}");
                pending.pop_front();
            }
        }
    }
}

fn handle_signal(rtc: &mut Rtc, signal: RtcSignal) -> Result<(), String> {
    match signal {
        RtcSignal::Offer(_) => {
            // Renegotiation isn't supported in this driver yet — the
            // browser only sends one offer per page load.
            warn!("RtcOffer received after init — ignoring");
        }
        RtcSignal::IceCandidate(candidate) => {
            // Browsers anonymise host candidates as `<uuid>.local`
            // mDNS hostnames for privacy. str0m doesn't resolve mDNS,
            // so those candidates fail to parse — that's expected,
            // and the connection still works because the browser has
            // the server's candidates (advertised in the SDP answer)
            // and connects to *those*.
            match Candidate::from_sdp_string(&candidate) {
                Ok(cand) => rtc.add_remote_candidate(cand),
                Err(e) => debug!("skipping unparseable remote candidate ({e}): {candidate}"),
            }
        }
    }
    Ok(())
}
