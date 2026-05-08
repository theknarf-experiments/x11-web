// Generates Rust types from `schema/ws.capnp` into
// `OUT_DIR/ws_capnp.rs`. `lib.rs` `include!`s the result so the
// generated module is a child of `ws_capnp` in our crate tree.

fn main() {
    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/ws.capnp")
        .run()
        .expect("capnp codegen failed");
    println!("cargo:rerun-if-changed=schema/ws.capnp");
}
