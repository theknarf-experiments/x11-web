//! Run the (patched) x11rb generator over the bundled xcb-proto XML files
//! at build time and drop the rendered Rust files into `$OUT_DIR/protocol/`.
//!
//! This crate exists as the build-time integration point for our endian-
//! aware fork of x11rb's code generator. The patch (in `patches/`) is
//! applied to the upstream generator by `patch-crate` before we link to it
//! here. This spike doesn't yet *consume* the generated code from elsewhere
//! in the workspace — it just confirms the round-trip works end-to-end.

use std::path::PathBuf;

fn main() {
    // The patched x11rb fork is materialised by `tools/setup-x11rb-fork.sh`,
    // which must have been run before this build. The workspace-root
    // `[patch."<git>"]` section already redirects Cargo at the patched
    // generator source, so by the time this build script runs, the call to
    // `x11rb_generator::run` below resolves to the patched API.
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let proto_out = out_dir.join("protocol");
    let x11rb_out = out_dir.join("x11rb-only");
    let async_out = out_dir.join("async-only");
    for d in [&proto_out, &x11rb_out, &async_out] {
        std::fs::create_dir_all(d).expect("create OUT_DIR subdir");
    }

    let xml_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("xcb-proto");

    println!("cargo:rerun-if-changed=patches");
    println!("cargo:rerun-if-changed=xcb-proto");

    x11rb_generator::run(&xml_dir, &proto_out, &x11rb_out, &async_out)
        .expect("x11rb-generator run failed");

    println!(
        "cargo:warning=x11rb-protocol-msb: generated {} files into {}",
        std::fs::read_dir(&proto_out)
            .map(|d| d.count())
            .unwrap_or(0),
        proto_out.display(),
    );
}
