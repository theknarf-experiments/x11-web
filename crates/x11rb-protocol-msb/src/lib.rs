//! Endian-aware fork of `x11rb-protocol`.
//!
//! For now this is a thin re-export of the upstream crate; the workspace
//! `[patch.crates-io]` redirects to our regenerated fork at
//! `tools/x11rb-fork/x11rb-protocol/` (see `tools/setup-x11rb-fork.sh`),
//! so this re-export already picks up any generator-side patches.
//!
//! Endian-aware traits and serialize helpers will land here as a layer
//! on top of `x11rb_protocol::*` in subsequent commits.

pub use x11rb_protocol::*;
