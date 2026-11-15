use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const CONFIG_FILE: &str = "config.json";
pub const VEHICLES_FILE: &str = "vehicles.json";
pub const UNKNOWN_FILE: &str = "unknown.txt";
pub const FLAGS_DIR: &str = "flags";
pub const SOUND_ENEMY: &str = "alert.mp3";
pub const SOUND_ALLY: &str = "alert_ally.mp3";
pub const DEBUG_SHOT: &str = "last_scan_debug.png";
pub const REPORT_FILE: &str = "last_scan_report.txt";
pub const RECORDINGS_DIR: &str = "recordings";

pub const EMBEDDED_VEHICLES: &str = include_str!("../vehicles.json");
pub const EMBEDDED_SOUND_ENEMY: &[u8] = include_bytes!("../alert.mp3");
pub const EMBEDDED_SOUND_ALLY: &[u8] = include_bytes!("../alert_ally.mp3");
include!(concat!(env!("OUT_DIR"), "/flags.rs"));

static BASE: OnceLock<PathBuf> = OnceLock::new();

pub fn base_dir() -> &'static Path {
    BASE.get_or_init(detect_base_dir)
}

pub fn file(name: &str) -> PathBuf {
    base_dir().join(name)
}

pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf))
}

/// The working copy first, then a read-only copy next to the executable
/// (e.g. the vehicles.json shipped in the release archive when that folder isn't writable).
pub fn find(name: &str) -> Option<PathBuf> {
    std::iter::once(file(name))
        .chain(exe_dir().map(|d| d.join(name)))
        .find(|p| p.exists())
}

pub fn read_or_embedded(name: &str, embedded: &'static [u8]) -> Vec<u8> {
    find(name).and_then(|p| std::fs::read(p).ok()).unwrap_or_else(|| embedded.to_vec())
}

fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".wt-tracker-write-test-{}", std::process::id()));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn detect_base_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("WT_TRACKER_DIR") {
        let dir = PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&dir);
        return dir;
    }
    let mut candidates = Vec::new();
    candidates.extend(exe_dir());
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }
    let found = candidates.iter().find(|c| c.join(CONFIG_FILE).exists() || c.join(VEHICLES_FILE).exists());
    pick_base_dir(found.map(PathBuf::as_path), user_data_dir(), candidates.first().map(PathBuf::as_path), is_writable)
}

// Program Files, /opt, a read-only AppImage mount: config and downloads would silently fail there
fn pick_base_dir(
    found: Option<&Path>,
    user_dir: Option<PathBuf>,
    fallback: Option<&Path>,
    writable: impl Fn(&Path) -> bool,
) -> PathBuf {
    if let Some(dir) = found {
        if writable(dir) {
            return dir.to_path_buf();
        }
        log::warn!("{} недоступна для записи, рабочая папка — пользовательская", dir.display());
    }
    let dir = user_dir.or_else(|| fallback.map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn user_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("wt-tracker"))
    }
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("WT Tracker"))
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readonly_program_dir_falls_back_to_user_dir() {
        let tmp = std::env::temp_dir().join(format!("wt-paths-{}", std::process::id()));
        let (prog, user) = (tmp.join("prog"), tmp.join("user"));
        std::fs::create_dir_all(&prog).unwrap();
        assert!(is_writable(&prog));
        assert!(!is_writable(&tmp.join("missing")));
        assert_eq!(pick_base_dir(Some(&prog), Some(user.clone()), None, |_| true), prog);
        assert_eq!(pick_base_dir(Some(&prog), Some(user.clone()), None, |_| false), user);
        assert!(user.is_dir());
        assert_eq!(pick_base_dir(None, Some(user.clone()), Some(&prog), |_| true), user);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
