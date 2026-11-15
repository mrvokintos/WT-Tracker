// Locating the local War Thunder install and reading files from its .vromfs.bin archives.
// Only used for the game's own symbol font, copied for this user's own install; nothing is redistributed.

use std::path::{Path, PathBuf};

fn is_game_dir(d: &Path) -> bool {
    d.join("ui/fonts.vromfs.bin").is_file()
}

/// `config_dir` (game.dir in config.json) first, then the running game, Steam libraries and the Gaijin launcher.
pub fn find_dir(config_dir: &str) -> Option<PathBuf> {
    let configured = Some(PathBuf::from(config_dir.trim())).filter(|p| !p.as_os_str().is_empty());
    configured.into_iter().chain(running()).chain(candidates()).find(|d| is_game_dir(d))
}

#[cfg(target_os = "linux")]
fn running() -> Option<PathBuf> {
    // .../War Thunder/linux64/aces
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let comm = std::fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
        if comm.trim() == "aces" {
            if let Ok(exe) = std::fs::read_link(entry.path().join("exe")) {
                if let Some(dir) = exe.parent().and_then(Path::parent) {
                    return Some(dir.to_path_buf());
                }
            }
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn running() -> Option<PathBuf> {
    None
}

fn candidates() -> Vec<PathBuf> {
    let mut steam_roots: Vec<PathBuf> = Vec::new();
    let mut out: Vec<PathBuf> = Vec::new();
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    if let Some(h) = &home {
        for p in [".local/share/Steam", ".steam/steam", ".steam/root", ".var/app/com.valvesoftware.Steam/.local/share/Steam"] {
            steam_roots.push(h.join(p));
        }
        out.push(h.join("WarThunder"));
        out.push(h.join(".local/share/WarThunder"));
        out.push(h.join("Games/WarThunder"));
    }
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(p) = std::env::var_os(var) {
            steam_roots.push(PathBuf::from(p).join("Steam"));
        }
    }
    if let Some(p) = std::env::var_os("LOCALAPPDATA") {
        out.push(PathBuf::from(p).join("WarThunder"));
    }
    let re = regex::Regex::new(r#""path"\s+"([^"]+)""#).unwrap();
    let mut libs = steam_roots.clone();
    for root in &steam_roots {
        if let Ok(vdf) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
            libs.extend(re.captures_iter(&vdf).map(|c| PathBuf::from(c[1].replace("\\\\", "\\"))));
        }
    }
    let mut res: Vec<PathBuf> = libs.iter().map(|l| l.join("steamapps/common/War Thunder")).collect();
    res.extend(out);
    res
}

/// One raw file from a vromfs archive, by inner path.
pub fn read_file(vromf: &Path, inner: &str) -> Option<Vec<u8>> {
    use wt_blk::vromf::{File, FileFilter, VromfUnpacker};
    let file = File::new(vromf).ok()?;
    let Ok(un) = VromfUnpacker::from_file(&file, false) else {
        log::warn!("can't read {}", vromf.display());
        return None;
    };
    let f = un.unpack_one(Path::new(inner), None, false, FileFilter::All).ok()?;
    Some(f.split().1)
}
