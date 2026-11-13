use std::{
    fs,
    path::{Path, PathBuf},
};
fn scan(root: &Path, path: &Path, out: &mut Vec<PathBuf>) {
    if !path.is_dir() {
        return;
    }
    for entry in fs::read_dir(path).expect("public vault assets") {
        let p = entry.expect("asset entry").path();
        if p.is_dir() {
            scan(root, &p, out)
        } else if p.is_file() {
            out.push(p.strip_prefix(root).unwrap().to_owned())
        }
    }
}
fn main() {
    let root =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../vault-web/dist");
    let mut paths = vec![];
    scan(&root, &root, &mut paths);
    paths.sort();
    let mut code = String::from("pub const ASSETS:&[(&str,&[u8])]=&[\n");
    for p in paths {
        let full = root.join(&p).canonicalize().unwrap();
        println!("cargo:rerun-if-changed={}", full.display());
        code.push_str(&format!(
            "({:?},include_bytes!({:?})),\n",
            format!("/{}", p.to_string_lossy()),
            full
        ));
    }
    code.push_str("];\n");
    println!("cargo:rerun-if-changed={}", root.display());
    fs::write(
        PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("assets.rs"),
        code,
    )
    .unwrap();
}
