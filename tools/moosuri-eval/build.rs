//! Bind reports to the source inputs, rather than an unchecked runtime checkout.
use sha2::{Digest, Sha256};
use std::{env, fs, path::Path};

fn collect(root: &Path, path: &Path, hash: &mut Sha256) {
    if path.is_dir() {
        let mut entries = fs::read_dir(path)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            collect(root, &entry, hash);
        }
    } else if matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("rs" | "toml" | "lock" | "sql" | "md" | "json")
    ) {
        let name = path.strip_prefix(root).unwrap().to_string_lossy();
        let bytes = fs::read(path).unwrap();
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
}
fn main() {
    let manifest = env::var_os("CARGO_MANIFEST_DIR").unwrap();
    let root = Path::new(&manifest).parent().unwrap().parent().unwrap();
    let mut hash = Sha256::new();
    for name in [
        "Cargo.toml",
        "Cargo.lock",
        "crates",
        "migrations",
        "community",
        "tools/moosuri-eval",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(name).display());
        collect(root, &root.join(name), &mut hash);
    }
    println!(
        "cargo:rustc-env=MOOSURI_EVAL_SOURCE_SHA256={:x}",
        hash.finalize()
    );
}
