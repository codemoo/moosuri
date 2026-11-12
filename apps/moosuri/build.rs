use std::{
    env, fs,
    path::{Path, PathBuf},
};
fn collect(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for item in entries.flatten() {
            let p = item.path();
            if p.is_symlink() {
                panic!("web assets must not be symlinks");
            }
            if p.is_dir() {
                collect(root, &p, out);
            } else if p.is_file() {
                out.push(p.strip_prefix(root).unwrap().to_path_buf());
            }
        }
    }
}
fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../../web/dist");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    collect(&root, &root, &mut files);
    files.sort();
    let mut generated = String::from("static ASSETS: &[(&str, &[u8])] = &[\n");
    for file in files {
        let path = root.join(&file).canonicalize().unwrap();
        generated.push_str(&format!(
            "({:?}, include_bytes!({:?})),\n",
            format!("/{}", file.display()),
            path.to_str().unwrap()
        ));
    }
    generated.push_str("];\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("assets.rs"),
        generated,
    )
    .unwrap();
}
