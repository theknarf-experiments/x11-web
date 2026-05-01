//! `dial` (sidecar) and `listen` (backend) plus the `Hello`
//! handshake.
//!
//! Each connection carries one persistent **bidi** stream. The
//! sidecar opens the stream, writes a `Hello`, and waits for a
//! `HelloAck`. From there both sides write framed Cap'n Proto
//! messages until either side hangs up.
//!
//! Single-stream model is the v0 simplification — split into
//! control / display / input streams later when we measure
//! head-of-line contention.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use capnp::message::ReaderOptions;
use quinn::{Endpoint, RecvStream, SendStream, TransportConfig};
use tracing::warn;

use crate::tls::{make_client_config, make_server_config, ServerCert};
use crate::wire_capnp;
use crate::{WireError, PROTOCOL_VERSION};

/// QUIC connection idle timeout — both sides drop the connection
/// when no packet has arrived in this long. Default Quinn is 30s,
/// which makes a killed sidecar take half a minute to disappear from
/// the dock; this gets it to a few seconds.
const QUIC_IDLE_TIMEOUT: Duration = Duration::from_secs(5);

/// QUIC keep-alive cadence — each side sends a PING frame this often
/// when otherwise idle, so an alive-but-quiet connection isn't
/// reaped by `QUIC_IDLE_TIMEOUT`. Must be < idle timeout / 2 so a
/// single dropped PING doesn't tear down a working connection.
const QUIC_KEEP_ALIVE: Duration = Duration::from_secs(2);

fn quic_transport_config() -> Arc<TransportConfig> {
    let mut t = TransportConfig::default();
    t.max_idle_timeout(Some(
        QUIC_IDLE_TIMEOUT
            .try_into()
            .expect("idle timeout fits in VarInt"),
    ));
    t.keep_alive_interval(Some(QUIC_KEEP_ALIVE));
    Arc::new(t)
}

/// Half of the bidi stream a peer reads from. Wraps `RecvStream` and
/// drains it with capnp's segment-aware reader.
pub struct WireReader {
    inner: RecvStream,
}

impl WireReader {
    /// Read one Cap'n Proto message off the stream. Returns
    /// `Ok(None)` on clean EOF.
    pub async fn read_message<T>(
        &mut self,
    ) -> Result<Option<capnp::message::Reader<capnp::serialize::OwnedSegments>>, WireError>
    where
        T: capnp::traits::Owned,
    {
        let _ = std::marker::PhantomData::<T>;
        // capnp's `read_message` takes a sync `Read`; we adapt by
        // pulling the message into memory via Tokio first. The
        // trade-off is one allocation per message — fine until we
        // start streaming raw frames, at which point we'll switch
        // to a streaming reader that knows the segment table.
        let bytes = match read_segment_framed(&mut self.inner).await? {
            Some(b) => b,
            None => return Ok(None),
        };
        let mut cursor = std::io::Cursor::new(bytes);
        let reader = capnp::serialize::read_message(&mut cursor, ReaderOptions::new())?;
        Ok(Some(reader))
    }
}

/// Half of the bidi stream a peer writes to.
pub struct WireWriter {
    inner: SendStream,
}

impl WireWriter {
    /// Encode + write a Cap'n Proto builder onto the stream. Uses
    /// capnp's standard segment-table framing (no extra
    /// length-prefix) — receivers use the segment table itself to
    /// find message boundaries.
    pub async fn write_message<A>(
        &mut self,
        message: &capnp::message::Builder<A>,
    ) -> Result<(), WireError>
    where
        A: capnp::message::Allocator,
    {
        let mut buf = Vec::new();
        capnp::serialize::write_message(&mut buf, message)?;
        self.inner
            .write_all(&buf)
            .await
            .map_err(|e| WireError::Connection(format!("write_all on QUIC stream failed: {e}")))?;
        Ok(())
    }
}

/// What the backend hands to its session handler after a successful
/// handshake. The reader/writer halves of the bidi stream are split
/// so the handler can drive recv + send concurrently.
pub struct AcceptedConnection {
    pub sidecar_id: String,
    pub sidecar_name: String,
    pub reader: WireReader,
    pub writer: WireWriter,
    /// quinn connection handle — kept so the caller can close it
    /// cleanly or open additional streams once we go multi-stream.
    pub connection: quinn::Connection,
}

/// What the sidecar gets back from `dial(...)` once the handshake
/// completed.
pub struct DialedConnection {
    pub sidecar_id: String,
    pub agreed_protocol_version: u32,
    pub reader: WireReader,
    pub writer: WireWriter,
    pub connection: quinn::Connection,
}

// ---------------------------------------------------------------------------
// Backend-side: listen.
// ---------------------------------------------------------------------------

/// Bind a QUIC listener at `bind` using the given self-signed
/// `ServerCert`. Returns the `Endpoint` ready to `accept()`
/// connections.
pub fn listen(bind: SocketAddr, cert: &ServerCert) -> Result<Endpoint, WireError> {
    let rustls_cfg = make_server_config(cert)?;
    let mut server_cfg = quinn::ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(rustls_cfg)
            .map_err(|e| WireError::Tls(format!("quic server config: {e}")))?,
    ));
    server_cfg.transport_config(quic_transport_config());
    let endpoint = Endpoint::server(server_cfg, bind)
        .map_err(|e| WireError::Connection(format!("endpoint bind {bind}: {e}")))?;
    Ok(endpoint)
}

/// Accept the next incoming connection from `endpoint`, drive the
/// `Hello` handshake, and return an `AcceptedConnection` ready for
/// the caller's session loop.
///
/// `validate_token` is called with the bearer-token bytes the
/// sidecar sent; returning `Err(_)` rejects the handshake. For v0
/// the backend usually wires this to a constant compare; v1 will
/// look up tokens in a database.
pub async fn accept(
    endpoint: &Endpoint,
    validate_token: impl Fn(&[u8], &str) -> Result<String, String>,
) -> Result<AcceptedConnection, WireError> {
    let incoming = endpoint
        .accept()
        .await
        .ok_or_else(|| WireError::Connection("endpoint closed".into()))?;
    let connection = incoming
        .await
        .map_err(|e| WireError::Connection(format!("connect: {e}")))?;

    // Sidecar opens the bidi stream, server accepts it.
    let (send, recv) = connection
        .accept_bi()
        .await
        .map_err(|e| WireError::Connection(format!("accept_bi: {e}")))?;
    let mut reader = WireReader { inner: recv };
    let mut writer = WireWriter { inner: send };

    // First message must be Hello.
    let hello_reader = reader
        .read_message::<wire_capnp::hello::Owned>()
        .await?
        .ok_or_else(|| WireError::Connection("client closed before Hello".into()))?;
    let hello: wire_capnp::hello::Reader = hello_reader
        .get_root()
        .map_err(|e| WireError::Connection(format!("Hello root: {e}")))?;

    let peer_version = hello.get_protocol_version();
    if peer_version != PROTOCOL_VERSION {
        // Send a Rejected ack before closing so the sidecar gets a
        // useful error instead of a bare connection drop.
        let mut nak = capnp::message::Builder::new_default();
        let mut ack = nak.init_root::<wire_capnp::hello_ack::Builder>();
        let mut rej = ack.reborrow().init_rejected();
        rej.set_message(&format!(
            "peer protocol version {peer_version}, server expects {PROTOCOL_VERSION}"
        ));
        let _ = writer.write_message(&nak).await;
        return Err(WireError::IncompatibleVersion {
            peer: peer_version,
            ours: PROTOCOL_VERSION,
        });
    }

    let token = hello
        .get_bearer_token()
        .map_err(|e| WireError::Connection(format!("bearer token read: {e}")))?;
    let sidecar_name = hello
        .get_sidecar_name()
        .map_err(|e| WireError::Connection(format!("sidecar name read: {e}")))?
        .to_string()
        .map_err(|e| WireError::Connection(format!("sidecar name utf8: {e}")))?;

    let sidecar_id = match validate_token(token, &sidecar_name) {
        Ok(id) => id,
        Err(reason) => {
            let mut nak = capnp::message::Builder::new_default();
            let mut ack = nak.init_root::<wire_capnp::hello_ack::Builder>();
            let mut rej = ack.reborrow().init_rejected();
            rej.set_message(&reason);
            let _ = writer.write_message(&nak).await;
            return Err(WireError::HandshakeRejected(reason));
        }
    };

    // Acknowledge.
    let mut ack_builder = capnp::message::Builder::new_default();
    {
        let ack = ack_builder.init_root::<wire_capnp::hello_ack::Builder>();
        let mut ok = ack.init_ok();
        ok.set_sidecar_id(&sidecar_id);
        ok.set_agreed_protocol_version(PROTOCOL_VERSION);
    }
    writer.write_message(&ack_builder).await?;

    Ok(AcceptedConnection {
        sidecar_id,
        sidecar_name,
        reader,
        writer,
        connection,
    })
}

// ---------------------------------------------------------------------------
// Sidecar-side: dial.
// ---------------------------------------------------------------------------

/// Dial the backend at `server_addr`, validate its certificate
/// against the pinned fingerprint, open the bidi stream, send
/// `Hello`, and wait for `HelloAck`.
pub async fn dial(
    server_addr: SocketAddr,
    server_name: &str,
    fingerprint: [u8; 32],
    bearer_token: &[u8],
    sidecar_name: &str,
) -> Result<DialedConnection, WireError> {
    let client_cfg = make_client_config(fingerprint)?;
    let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())
        .map_err(|e| WireError::Connection(format!("client endpoint: {e}")))?;
    let mut quic_client_cfg = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(client_cfg)
            .map_err(|e| WireError::Tls(format!("quic client config: {e}")))?,
    ));
    quic_client_cfg.transport_config(quic_transport_config());
    endpoint.set_default_client_config(quic_client_cfg);

    let connection = endpoint
        .connect(server_addr, server_name)
        .map_err(|e| WireError::Connection(format!("connect setup: {e}")))?
        .await
        .map_err(|e| WireError::Connection(format!("connect handshake: {e}")))?;

    let (send, recv) = connection
        .open_bi()
        .await
        .map_err(|e| WireError::Connection(format!("open_bi: {e}")))?;
    let mut reader = WireReader { inner: recv };
    let mut writer = WireWriter { inner: send };

    // Send Hello.
    let mut hello_builder = capnp::message::Builder::new_default();
    {
        let mut hello = hello_builder.init_root::<wire_capnp::hello::Builder>();
        hello.set_protocol_version(PROTOCOL_VERSION);
        hello.set_bearer_token(bearer_token);
        hello.set_sidecar_name(sidecar_name);
    }
    writer.write_message(&hello_builder).await?;

    // Read HelloAck.
    let ack_reader = reader
        .read_message::<wire_capnp::hello_ack::Owned>()
        .await?
        .ok_or_else(|| WireError::Connection("server closed before HelloAck".into()))?;
    let ack: wire_capnp::hello_ack::Reader = ack_reader
        .get_root()
        .map_err(|e| WireError::Connection(format!("HelloAck root: {e}")))?;

    use wire_capnp::hello_ack::Which;
    match ack
        .which()
        .map_err(|e| WireError::Connection(format!("HelloAck variant: {e}")))?
    {
        Which::Ok(ok) => {
            let ok = ok.map_err(|e| WireError::Connection(format!("HelloAck.ok: {e}")))?;
            let id = ok
                .get_sidecar_id()
                .map_err(|e| WireError::Connection(format!("sidecar_id read: {e}")))?
                .to_string()
                .map_err(|e| WireError::Connection(format!("sidecar_id utf8: {e}")))?;
            let agreed = ok.get_agreed_protocol_version();
            Ok(DialedConnection {
                sidecar_id: id,
                agreed_protocol_version: agreed,
                reader,
                writer,
                connection,
            })
        }
        Which::Rejected(rej) => {
            let rej = rej.map_err(|e| WireError::Connection(format!("HelloAck.rejected: {e}")))?;
            let msg = rej
                .get_message()
                .map_err(|e| WireError::Connection(format!("rejected message: {e}")))?
                .to_string()
                .map_err(|e| WireError::Connection(format!("rejected message utf8: {e}")))?;
            Err(WireError::HandshakeRejected(msg))
        }
    }
}

// ---------------------------------------------------------------------------
// Streaming framing helper.
// ---------------------------------------------------------------------------

/// Read one capnp message's worth of bytes off the stream. Cap'n
/// Proto's wire format begins with a segment table: 4-byte
/// `(segment_count - 1)`, then 4 bytes `(segment_size_in_words)`
/// for each segment, padded to 8-byte boundary, then the segments.
/// We read just enough to know the total message size, then read
/// the rest in one go.
async fn read_segment_framed(stream: &mut RecvStream) -> Result<Option<Vec<u8>>, WireError> {
    // First word: 4-byte little-endian (segment_count - 1) + 4-byte
    // first segment's size in 8-byte words.
    let mut header = [0u8; 8];
    if !read_exact_or_eof(stream, &mut header).await? {
        return Ok(None);
    }
    let seg_count =
        u32::from_le_bytes([header[0], header[1], header[2], header[3]]).saturating_add(1) as usize;
    let mut seg_sizes = vec![0u32; seg_count];
    seg_sizes[0] = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);

    // If there are 2+ segments, the table continues. Total table
    // bytes (excluding the first 8 we already read) is
    // `4 * (seg_count - 1)` rounded up to 8-byte boundary.
    if seg_count > 1 {
        let extra_table_bytes = (seg_count - 1) * 4;
        let padded = if extra_table_bytes % 8 == 0 {
            extra_table_bytes
        } else {
            extra_table_bytes + (8 - extra_table_bytes % 8)
        };
        let mut rest_table = vec![0u8; padded];
        if !read_exact_or_eof(stream, &mut rest_table).await? {
            return Err(WireError::Connection("EOF mid-segment-table".into()));
        }
        for i in 0..(seg_count - 1) {
            let off = i * 4;
            seg_sizes[i + 1] = u32::from_le_bytes([
                rest_table[off],
                rest_table[off + 1],
                rest_table[off + 2],
                rest_table[off + 3],
            ]);
        }
    }

    let total_words: u64 = seg_sizes.iter().map(|s| *s as u64).sum();
    let body_bytes = (total_words * 8) as usize;
    let mut body = vec![0u8; body_bytes];
    if !read_exact_or_eof(stream, &mut body).await? {
        return Err(WireError::Connection("EOF mid-message-body".into()));
    }

    // Reassemble for capnp::serialize::read_message: header table +
    // body.
    let mut all = Vec::with_capacity(8 + body.len() + (seg_count.saturating_sub(1)) * 4 + 4);
    all.extend_from_slice(&header);
    if seg_count > 1 {
        // We read the rest of the table above. Re-derive what to put
        // back: write each remaining size as little-endian u32 plus
        // padding to 8-byte boundary.
        for i in 1..seg_count {
            all.extend_from_slice(&seg_sizes[i].to_le_bytes());
        }
        let so_far = 8 + (seg_count - 1) * 4;
        let pad = if so_far % 8 == 0 { 0 } else { 8 - so_far % 8 };
        all.extend(std::iter::repeat(0u8).take(pad));
    }
    all.extend_from_slice(&body);
    Ok(Some(all))
}

/// Wrapper around `read_exact` that distinguishes "clean EOF before
/// any byte arrived" from "EOF mid-buffer."
async fn read_exact_or_eof(stream: &mut RecvStream, buf: &mut [u8]) -> Result<bool, WireError> {
    let mut filled = 0;
    while filled < buf.len() {
        match stream.read(&mut buf[filled..]).await {
            Ok(Some(0)) | Ok(None) => {
                if filled == 0 {
                    return Ok(false);
                }
                return Err(WireError::Connection(format!(
                    "EOF after {filled}/{} bytes",
                    buf.len()
                )));
            }
            Ok(Some(n)) => filled += n,
            Err(e) => {
                warn!("QUIC recv error: {e}");
                return Err(WireError::Connection(format!("recv: {e}")));
            }
        }
    }
    Ok(true)
}
