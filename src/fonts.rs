// egui ships Latin/Cyrillic fonts and emoji only: CJK nicknames and the vehicle markers used in
// names (◥ ◍ ▄ ☢ ...) render as boxes. The markers are icons from the game's own symbol font,
// read once from the local install and cached in the working folder (never distributed with the
// tracker). System fonts cover CJK and whatever the game font doesn't have.

use egui::{FontData, FontDefinitions, FontFamily};
use std::collections::BTreeSet;
use std::path::PathBuf;

pub const SYMBOL_FONT: &str = "symbols_skyquake.ttf";
const SYMBOL_FONT_IN_GAME: &str = "ttfs/symbols_skyquake.ttf";

/// Symbol characters the vehicle names use (arrows, box drawing, blocks, geometric shapes, misc symbols).
fn name_symbols() -> BTreeSet<char> {
    crate::paths::EMBEDDED_VEHICLES.chars().filter(|&c| ('\u{2190}'..='\u{2BFF}').contains(&c)).collect()
}

fn has_glyph(font: &[u8], c: char) -> bool {
    ttf_parser::Face::parse(font, 0).ok().and_then(|f| f.glyph_index(c)).is_some()
}

/// The cached copy in the working folder (or next to the program), else read it from the game
/// once and cache it. None when the game isn't installed.
fn symbol_font() -> Option<Vec<u8>> {
    if let Some(bytes) = crate::paths::find(SYMBOL_FONT).and_then(|p| std::fs::read(p).ok()) {
        if ttf_parser::Face::parse(&bytes, 0).is_ok() {
            return Some(bytes);
        }
        log::warn!("{SYMBOL_FONT} is damaged, reading it from the game again");
    }
    let cfg = crate::config::Config::load(&crate::paths::file(crate::paths::CONFIG_FILE));
    let Some(dir) = crate::game::find_dir(&cfg.game.dir) else {
        log::info!("War Thunder not found: vehicle markers use system fonts");
        return None;
    };
    let bytes = crate::game::read_file(&dir.join("ui/fonts.vromfs.bin"), SYMBOL_FONT_IN_GAME)?;
    ttf_parser::Face::parse(&bytes, 0).ok()?;
    let path = crate::paths::file(SYMBOL_FONT);
    match std::fs::write(&path, &bytes) {
        Ok(()) => log::info!("vehicle marker font read from {} and saved to {}", dir.display(), path.display()),
        Err(e) => log::warn!("can't save {}: {e}", path.display()),
    }
    Some(bytes)
}

#[cfg(target_os = "linux")]
fn candidates(symbols: &BTreeSet<char>) -> Vec<(PathBuf, u32)> {
    let fc = |pattern: &str| -> Option<(PathBuf, u32)> {
        let out = std::process::Command::new("fc-match").args(["-f", "%{file}\n%{index}", pattern]).output().ok()?;
        let text = String::from_utf8(out.stdout).ok()?;
        let mut lines = text.lines();
        let file = PathBuf::from(lines.next()?);
        let index = lines.next().and_then(|i| i.parse().ok()).unwrap_or(0);
        file.is_file().then_some((file, index))
    };
    let mut out: Vec<(PathBuf, u32)> = ["sans-serif:lang=zh-cn", "sans-serif:lang=ja", "sans-serif:lang=ko"]
        .into_iter()
        .filter_map(fc)
        .collect();
    out.extend(symbols.iter().filter_map(|c| fc(&format!(":charset={:x}", *c as u32))));
    if out.is_empty() {
        // no fontconfig: common locations
        for p in [
            "/usr/share/fonts/google-noto-sans-cjk-fonts/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
        ] {
            let p = PathBuf::from(p);
            if p.is_file() {
                out.push((p, 0));
            }
        }
    }
    out
}

#[cfg(windows)]
fn candidates(_symbols: &BTreeSet<char>) -> Vec<(PathBuf, u32)> {
    let dir = PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
    // Chinese, Japanese, Korean, then symbols
    ["msyh.ttc", "YuGothR.ttc", "meiryo.ttc", "malgun.ttf", "seguisym.ttf"]
        .into_iter()
        .map(|f| (dir.join(f), 0))
        .filter(|(p, _)| p.is_file())
        .collect()
}

#[cfg(not(any(target_os = "linux", windows)))]
fn candidates(_symbols: &BTreeSet<char>) -> Vec<(PathBuf, u32)> {
    vec![]
}

pub fn install(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let mut missing = name_symbols();
    if let Some(bytes) = symbol_font() {
        // first: it has no letters, only the markers
        missing.retain(|&c| !has_glyph(&bytes, c));
        defs.font_data.insert("wt-symbols".into(), FontData::from_owned(bytes));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            defs.families.entry(family).or_default().insert(0, "wt-symbols".into());
        }
    }
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    for (path, index) in candidates(&missing) {
        // one face per file: every face of a CJK collection has the ideographs, no need for three copies
        if !seen.insert(path.clone()) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let name = format!("fallback-{}", seen.len());
        let mut data = FontData::from_owned(bytes);
        data.index = index;
        defs.font_data.insert(name.clone(), data);
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            defs.families.entry(family).or_default().push(name.clone());
        }
        log::debug!("fallback font {} [{index}]", path.display());
    }
    ctx.set_fonts(defs);
}

#[cfg(test)]
mod tests {
    #[test]
    fn vehicle_symbols_are_collected() {
        let s = super::name_symbols();
        assert!(s.contains(&'◍') && s.contains(&'▄') && s.contains(&'☢'), "{s:?}");
    }

    /// Only where War Thunder is installed (not in CI): the font comes out of the game archive.
    #[test]
    fn symbol_font_from_installed_game() {
        let Some(dir) = crate::game::find_dir("") else { return };
        let font = crate::game::read_file(&dir.join("ui/fonts.vromfs.bin"), super::SYMBOL_FONT_IN_GAME).expect("font in the archive");
        for c in ['◥', '◍', '▄', '☢'] {
            assert!(super::has_glyph(&font, c), "{c}");
        }
        assert!(!super::has_glyph(&font, 'A'), "symbols only");
    }
}
