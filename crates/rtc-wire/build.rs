// Generates Rust types from `schema/wire.capnp` into
// `OUT_DIR/wire_capnp.rs`. `lib.rs` `include!`s the result so the
// generated module is a child of `wire_capnp` in our crate tree.

fn main() {
    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/wire.capnp")
        .run()
        .expect("capnp codegen failed");
    // Re-run when the schema changes.
    println!("cargo:rerun-if-changed=schema/wire.capnp");
}
