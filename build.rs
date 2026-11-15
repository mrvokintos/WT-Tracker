// Embeds every flags/*.png as a scan template, so adding a flag doesn't need a code change.
use std::fmt::Write;

fn main() {
    println!("cargo:rerun-if-changed=flags");
    let dir = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("flags");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("flags/")
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".png"))
        .collect();
    names.sort();
    let mut out = String::from("pub const EMBEDDED_FLAGS: &[(&str, &[u8])] = &[\n");
    for n in names {
        let path = dir.join(&n);
        writeln!(out, "    ({:?}, include_bytes!({:?})),", n.trim_end_matches(".png"), path.display().to_string()).unwrap();
    }
    out.push_str("];\n");

    std::fs::write(std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("flags.rs"), out).unwrap();
}
