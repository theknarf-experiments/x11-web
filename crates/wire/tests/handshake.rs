//! End-to-end smoke test: stand up a backend listener, dial it
//! from a "sidecar" client, exchange Hello/HelloAck, then send a
//! heartbeat and observe it on the server side.
//!
//! Catches regressions in the QUIC plumbing + Cap'n Proto framing
//! without dragging the actual sidecar binary into the test.

use std::time::Duration;

use x11_web_wire::conn::{accept, dial, listen, SidecarKind};
use x11_web_wire::tls::{generate_self_signed, parse_fingerprint};
use x11_web_wire::{wire_capnp, PROTOCOL_VERSION};

#[tokio::test]
async fn handshake_and_heartbeat_roundtrip() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let cert = generate_self_signed(vec!["localhost".into()]).expect("gen cert");
    let fingerprint = parse_fingerprint(&cert.fingerprint_hex()).expect("parse fingerprint");
    let endpoint = listen("127.0.0.1:0".parse().unwrap(), &cert).expect("listen");
    let server_addr = endpoint.local_addr().expect("local addr");

    // Drive both sides concurrently with `join!` rather than
    // `spawn` — capnp message readers contain raw pointer types
    // that aren't `Send`, and `tokio::spawn` rejects non-Send
    // futures. `join!` runs futures in the same task so no Send.
    let server = async {
        let mut accepted = accept(&endpoint, |token, name| {
            assert_eq!(token, b"test-token");
            assert_eq!(name, "test-sidecar");
            Ok(format!("sid-for-{name}"))
        })
        .await
        .expect("accept");
        assert_eq!(accepted.sidecar_name, "test-sidecar");
        assert_eq!(accepted.sidecar_id, "sid-for-test-sidecar");

        let msg = accepted
            .reader
            .read_message::<wire_capnp::from_sidecar::Owned>()
            .await
            .expect("read")
            .expect("expected one message before EOF");
        let from: wire_capnp::from_sidecar::Reader = msg.get_root().unwrap();
        let is_heartbeat = matches!(
            from.which().expect("which"),
            wire_capnp::from_sidecar::Which::Heartbeat(()),
        );
        assert!(is_heartbeat, "expected heartbeat");
    };

    let client = async {
        let mut dialed = dial(
            server_addr,
            "localhost",
            fingerprint,
            b"test-token",
            "test-sidecar",
            SidecarKind::X11,
        )
        .await
        .expect("dial");
        assert_eq!(dialed.agreed_protocol_version, PROTOCOL_VERSION);

        let mut hb = capnp::message::Builder::new_default();
        {
            let mut from = hb.init_root::<wire_capnp::from_sidecar::Builder>();
            from.set_heartbeat(());
        }
        dialed.writer.write_message(&hb).await.expect("write hb");

        // Hold the connection open briefly so the server has time
        // to read the heartbeat. Dropping `dialed` here would
        // close the QUIC connection before the heartbeat lands.
        tokio::time::sleep(Duration::from_millis(100)).await;
    };

    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(server, client);
    })
    .await
    .expect("test timed out");
}
