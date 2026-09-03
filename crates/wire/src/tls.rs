//! Self-signed TLS for the QUIC connection, with SHA-256
//! fingerprint pinning for v0.
//!
//! Backend side:
//!   - `generate_self_signed(...)` produces a fresh cert + key on
//!     startup and exposes the cert's SHA-256 fingerprint.
//!   - `make_server_config(...)` wraps cert + key into a
//!     `rustls::ServerConfig` quinn accepts.
//!
//! Sidecar side:
//!   - `make_client_config(fingerprint)` returns a
//!     `rustls::ClientConfig` whose certificate verifier accepts
//!     **any** cert whose SHA-256 fingerprint matches the
//!     operator-supplied value. No CA, no DNS validation —
//!     pin-by-fingerprint is the entire trust decision.
//!
//! Trust model rationale: the backend's cert never roots in a CA
//! the sidecar has on its trust store. Operators get a one-time
//! fingerprint when they spin up the backend, paste it into the
//! sidecar config, and that's the entire ceremony. v1 will replace
//! this with proper certs once the backend has a stable address +
//! Let's Encrypt.

use std::sync::Arc;

use rcgen::CertifiedKey;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as TlsError, SignatureScheme};
use sha2::{Digest, Sha256};

use crate::WireError;

/// ALPN identifier our QUIC connections advertise. ALPN matching is
/// required by quinn — both sides must agree on the same byte
/// string. Bumping the protocol version doesn't necessarily bump
/// this; the version is negotiated inside the `Hello` message.
pub const ALPN: &[u8] = b"x11-web/1";

/// Server certificate + key + the SHA-256 fingerprint clients can
/// pin against. The backend prints `fingerprint_hex()` on startup
/// and operators paste it into sidecar configs.
///
/// Not `Clone` — `PrivateKeyDer` deliberately isn't `Clone` to
/// reduce key-copying surface. Use `clone_key()` for explicit
/// copies when needed.
pub struct ServerCert {
    pub cert_der: CertificateDer<'static>,
    pub key_der: PrivateKeyDer<'static>,
    pub fingerprint: [u8; 32],
}

impl ServerCert {
    /// Display form of the fingerprint, e.g.
    /// `"a3:c2:7e:..."`. Same byte order as `openssl x509
    /// -fingerprint -sha256` so operators can copy-paste between
    /// our log and other tools.
    pub fn fingerprint_hex(&self) -> String {
        let mut out = String::with_capacity(32 * 3);
        for (i, b) in self.fingerprint.iter().enumerate() {
            if i > 0 {
                out.push(':');
            }
            out.push_str(&format!("{b:02x}"));
        }
        out
    }
}

/// Generate a fresh ECDSA P-256 self-signed cert valid for the
/// supplied DNS-style names. The cert's `Subject` is whatever
/// `rcgen` defaults to — irrelevant since the sidecar verifier
/// pins on the cert hash, not on names.
pub fn generate_self_signed(subject_alt_names: Vec<String>) -> Result<ServerCert, WireError> {
    let CertifiedKey { cert, key_pair } = rcgen::generate_simple_self_signed(subject_alt_names)
        .map_err(|e| WireError::Tls(format!("cert generation failed: {e}")))?;

    let cert_der_bytes = cert.der().to_vec();
    let key_pem = key_pair.serialize_pem();
    let key_pkcs8 = pem_to_pkcs8(&key_pem)?;

    let mut hasher = Sha256::new();
    hasher.update(&cert_der_bytes);
    let fingerprint: [u8; 32] = hasher.finalize().into();

    Ok(ServerCert {
        cert_der: CertificateDer::from(cert_der_bytes),
        key_der: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pkcs8)),
        fingerprint,
    })
}

/// Decode rcgen's PEM private-key string into the raw PKCS#8 DER
/// bytes rustls wants.
fn pem_to_pkcs8(pem: &str) -> Result<Vec<u8>, WireError> {
    let body: String = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    use base64_decode_compat::decode as b64;
    b64(body.as_bytes())
        .map_err(|e| WireError::Tls(format!("private key base64 decode failed: {e}")))
}

/// Build a `rustls::ServerConfig` from a generated `ServerCert`.
/// quinn wraps this into its own server config.
pub fn make_server_config(cert: &ServerCert) -> Result<rustls::ServerConfig, WireError> {
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert.cert_der.clone()], cert.key_der.clone_key())
        .map_err(|e| WireError::Tls(format!("server config build failed: {e}")))?;
    config.alpn_protocols = vec![ALPN.to_vec()];
    Ok(config)
}

/// Sidecar-side: build a `rustls::ClientConfig` that accepts any
/// cert whose SHA-256 fingerprint matches `expected`.
pub fn make_client_config(expected: [u8; 32]) -> Result<rustls::ClientConfig, WireError> {
    let mut config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(FingerprintVerifier { expected }))
        .with_no_client_auth();
    config.alpn_protocols = vec![ALPN.to_vec()];
    Ok(config)
}

/// Parse a `aa:bb:cc:..` colon-separated hex fingerprint into the
/// raw 32-byte hash. Whitespace and a leading `sha256:` are
/// tolerated so operator copy-paste is forgiving.
pub fn parse_fingerprint(input: &str) -> Result<[u8; 32], WireError> {
    let trimmed = input
        .trim()
        .trim_start_matches("sha256:")
        .trim_start_matches("SHA256:")
        .replace([':', ' '], "");
    let bytes = hex::decode(&trimmed)
        .map_err(|e| WireError::Tls(format!("fingerprint hex decode failed: {e}")))?;
    if bytes.len() != 32 {
        return Err(WireError::Tls(format!(
            "fingerprint must be 32 bytes; got {}",
            bytes.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// rustls verifier that accepts any cert whose SHA-256 fingerprint
/// matches `expected`. Implementation is deliberately minimal —
/// no CA chain check, no DNS name match. Trust is `expected`.
#[derive(Debug)]
struct FingerprintVerifier {
    expected: [u8; 32],
}

impl ServerCertVerifier for FingerprintVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        let mut hasher = Sha256::new();
        hasher.update(end_entity.as_ref());
        let actual: [u8; 32] = hasher.finalize().into();
        if actual == self.expected {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(TlsError::General(
                "server cert fingerprint did not match pinned value".into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        // rustls calls this to figure out which signature schemes
        // it can negotiate. We accept the modern set; older ones
        // are unlikely to be used between our own peers.
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ED25519,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
        ]
    }
}

// Local base64 decoder — pulled in to avoid taking a dependency on
// the `base64` crate just for this one parse step. PEM bodies use
// the standard alphabet with no URL-safe variants.
mod base64_decode_compat {
    pub fn decode(input: &[u8]) -> Result<Vec<u8>, String> {
        // Strip newlines / whitespace.
        let input: Vec<u8> = input
            .iter()
            .copied()
            .filter(|b| !b.is_ascii_whitespace())
            .collect();
        let n_pad = input.iter().rev().take_while(|&&b| b == b'=').count();
        let useful = &input[..input.len() - n_pad];
        let mut out = Vec::with_capacity(useful.len() * 3 / 4);
        let mut buf: u32 = 0;
        let mut bits: u32 = 0;
        for &b in useful {
            let v = match b {
                b'A'..=b'Z' => b - b'A',
                b'a'..=b'z' => b - b'a' + 26,
                b'0'..=b'9' => b - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => return Err(format!("invalid base64 char {b:#x}")),
            } as u32;
            buf = (buf << 6) | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buf >> bits) as u8);
                buf &= (1 << bits) - 1;
            }
        }
        Ok(out)
    }
}
