//! Endian-aware fork of x11rb-protocol, generated at build time from a
//! patched x11rb generator. See `build.rs` for the round-trip.
//!
//! This file is a placeholder for the spike. The next steps will:
//! - patch the generator to thread `ByteOrder` through `TryParse` /
//!   `Serialize` emission;
//! - vendor (or pull in) x11rb-protocol's framework half (`x11_utils`,
//!   `errors`, etc.) so the generated module compiles standalone;
//! - re-export the resulting `protocol::*` from this crate.

/// Path to the directory of generated `.rs` files. Exposed so consumers (or
/// integration tests) can sanity-check the build artefact.
pub const GENERATED_DIR: &str = concat!(env!("OUT_DIR"), "/protocol");
