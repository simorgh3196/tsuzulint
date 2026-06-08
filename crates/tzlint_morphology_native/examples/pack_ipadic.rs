//! Build a distributable dictionary container from lindera's embedded IPADIC.
//!
//! A **maintainer tool**, never shipped: it produces the *uncompressed* `.dict` container that
//! [`LinderaProvider::from_dictionary_bytes`](tzlint_morphology_native::LinderaProvider::from_dictionary_bytes)
//! consumes. Compress and pin it out of band (the in-tree `ruzstd` decoder is decode-only, so
//! packaging shells out to the `zstd` CLI):
//!
//! ```sh
//! cargo run -p tzlint_morphology_native --example pack_ipadic --features embed-ipadic -- ipadic.dict
//! zstd -q -19 ipadic.dict -o ipadic.dict.zst   # compress (any zstd level)
//! b3sum ipadic.dict.zst                         # the BLAKE3 pin for `.tzlintrc` morphology.pin
//! ```
//!
//! The config `pin` is BLAKE3 over the **compressed** `ipadic.dict.zst` (the value
//! `provision_dictionary` verifies). Re-pack and re-pin on any lindera-dictionary version bump:
//! the container embeds version-coupled rkyv/daachorse blobs.

use std::error::Error;
use std::path::Path;

use lindera::dictionary::load_dictionary;
use tzlint_core::dict::container;
use tzlint_core::io::{Host, NativeHost};
use tzlint_morphology_native::extract_components;

fn main() -> Result<(), Box<dyn Error>> {
    let out = std::env::args()
        .nth(1)
        .ok_or("usage: pack_ipadic <out.dict>")?;

    let dictionary = load_dictionary("embedded://ipadic").map_err(|e| e.to_string())?;
    let components = extract_components(&dictionary).map_err(|e| e.to_string())?;
    let refs: [&[u8]; container::MEMBER_COUNT] = std::array::from_fn(|i| components[i].as_slice());
    let blob = container::encode(&refs).map_err(|e| e.to_string())?;

    // Write through the centralized Host boundary (atomic temp-then-rename), the same way the
    // linter does all of its I/O — so an interrupted pack never leaves a half-written `.dict`.
    NativeHost
        .write_atomic(Path::new(&out), &blob)
        .map_err(|e| e.to_string())?;

    println!("wrote {out} ({} bytes, uncompressed container)", blob.len());
    println!("next: `zstd -q -19 {out} -o {out}.zst` then `b3sum {out}.zst` for the config pin");
    Ok(())
}
