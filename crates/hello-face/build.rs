// Record build compatibility without exposing compiler flags in diagnostics.
use std::{env, process::Command};
fn main() {
    let rustc = env::var_os("RUSTC").expect("Cargo supplies RUSTC");
    let output = Command::new(rustc)
        .arg("--version")
        .output()
        .expect("read compiler version");
    assert!(
        output.status.success() && output.stdout.len() < 4096,
        "invalid compiler identity"
    );
    let mut identity = output.stdout;
    for name in [
        "TARGET",
        "OPT_LEVEL",
        "DEBUG",
        "CARGO_CFG_TARGET_FEATURE",
        "CARGO_ENCODED_RUSTFLAGS",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
        let value = env::var(name).unwrap_or_default();
        identity.extend_from_slice(&(value.len() as u64).to_le_bytes());
        identity.extend_from_slice(value.as_bytes());
    }
    let encoded: String = identity.iter().map(|b| format!("{b:02x}")).collect();
    println!("cargo:rustc-env=HELLO_FACE_BUILD={encoded}");
    println!("cargo:rerun-if-changed=build.rs");
}
