//! Generate OpenGL bindings via `gl_generator`.
//!
//! Compatibility profile so legacy fixed-function GL (the part
//! `gl_bindings.rs` wraps) is included — `Begin`, `End`, `Vertex*`,
//! `Color*`, `MatrixMode`, `LoadIdentity`, `CallList`, etc. The `gl`
//! crate generates core-only and would miss all of those.
//!
//! Output lives in `$OUT_DIR/gl_bindings_generated.rs` and is
//! `include!()`d from `osmesa/gl_generated.rs`.

use std::env;
use std::fs::File;
use std::path::PathBuf;

use gl_generator::{Api, Fallbacks, GlobalGenerator, Profile, Registry};

fn main() {
    let dest = PathBuf::from(env::var("OUT_DIR").unwrap()).join("gl_bindings_generated.rs");
    let mut file = File::create(&dest).unwrap();

    Registry::new(Api::Gl, (4, 6), Profile::Compatibility, Fallbacks::All, [])
        .write_bindings(GlobalGenerator, &mut file)
        .unwrap();

    println!("cargo:rerun-if-changed=build.rs");
}
