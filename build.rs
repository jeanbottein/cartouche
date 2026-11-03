//! Embed an optional default SteamGridDB API key, XOR-obfuscated so `strings`
//! can't lift it from the binary. Set `STEAMGRIDDB_API_KEY` at build time (CI
//! does, from a secret); without it the binary ships with no default key.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=STEAMGRIDDB_API_KEY");
    let key = std::env::var("STEAMGRIDDB_API_KEY").unwrap_or_default();
    let key = key.trim().as_bytes();
    let nonce: Vec<u8> = (0..key.len()).map(|_| random_byte()).collect();
    let encrypted: Vec<u8> = key.iter().zip(&nonce).map(|(k, n)| k ^ n).collect();

    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("api_key.rs");
    let source = format!("const KEY: &[u8] = &{encrypted:?};\nconst NONCE: &[u8] = &{nonce:?};\n");
    std::fs::write(out, source).unwrap();
}

/// Obfuscation only needs unpredictability, not cryptographic strength.
fn random_byte() -> u8 {
    RandomState::new().build_hasher().finish() as u8
}
