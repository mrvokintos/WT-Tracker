use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

pub const NATIONS: &[&str] =
    &["USA", "Germany", "USSR", "Great Britain", "Japan", "China", "Italy", "France", "Sweden", "Israel"];

pub const TYPES: &[&str] = &["tank", "plane", "heli", "zsu", "special"];

pub fn nation_display(key: &str) -> String {
    let s = crate::i18n::t(&format!("nation.{key}"));
    if s.starts_with("nation.") { key.to_string() } else { s }
}

pub fn type_display(key: &str) -> String {
    crate::i18n::t(&format!("type.{key}"))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub overlay: OverlayCfg,
    pub filters: Filters,
    pub scanner: ScannerCfg,
    pub hotkeys: Hotkeys,
    pub display: Display,
    pub audio: Audio,
    pub record: Record,
    pub db: DbCfg,
    pub ui: UiCfg,
    pub player: PlayerCfg,
    pub game: GameCfg,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OverlayCfg {
    pub x: i32,
    pub y: i32,
    pub size: u32,
    pub opacity: u32,
    pub monitor_idx: i32,
    pub gamescope: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Filters {
    pub nations: Vec<String>,
    pub types: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ScannerCfg {
    pub region: Option<[i32; 4]>,
    pub confidence_threshold: f32,
    pub hotkey: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Hotkeys {
    pub scan: String,
    pub overlay_toggle: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Display {
    pub show_notes: bool,
    pub show_names: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GameCfg {
    /// War Thunder folder; empty = find it automatically (used for the game's symbol font)
    pub dir: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PlayerCfg {
    /// in-game nickname without the clan tag; anchors side detection in mixed battles
    pub nickname: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UiCfg {
    /// "auto" or a translation code from lang/
    pub language: String,
    /// Linux window mode on startup: "x11" (XWayland) or "wayland"
    pub backend: String,
}

impl Default for UiCfg {
    fn default() -> Self {
        Self { language: "auto".into(), backend: "x11".into() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DbCfg {
    pub auto_update: bool,
    pub url: String,
}

impl Default for DbCfg {
    fn default() -> Self {
        Self { auto_update: true, url: crate::dbupdate::DEFAULT_URL.into() }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Record {
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Audio {
    pub enabled: bool,
}

impl Default for OverlayCfg {
    fn default() -> Self {
        Self { x: 30, y: 150, size: 20, opacity: 100, monitor_idx: 0, gamescope: true }
    }
}

impl Default for Filters {
    fn default() -> Self {
        Self { nations: vec![], types: TYPES.iter().map(|k| k.to_string()).collect() }
    }
}

impl Default for ScannerCfg {
    fn default() -> Self {
        Self { region: None, confidence_threshold: 0.70, hotkey: "\\".into() }
    }
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self { scan: "\\".into(), overlay_toggle: "f5".into() }
    }
}

impl Default for Display {
    fn default() -> Self {
        Self { show_notes: true, show_names: true }
    }
}

impl Default for Audio {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl Config {
    pub fn load(path: &Path) -> Config {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).unwrap_or_else(|e| {
                log::error!("config.json повреждён ({e}), используются настройки по умолчанию");
                Config::default()
            }),
            Err(_) => Config::default(),
        }
    }

    pub fn parse(text: &str) -> Result<Config, serde_json::Error> {
        let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
        let mut cfg: Config = serde_json::from_value(v.clone())?;
        if v.get("overlay").is_none() {
            apply_legacy(&mut cfg, &v);
        }
        if v.pointer("/hotkeys/scan").is_none() {
            if let Some(k) = v.pointer("/scanner/hotkey").and_then(Value::as_str) {
                cfg.hotkeys.scan = k.to_string();
            }
        }
        cfg.scanner.hotkey = cfg.hotkeys.scan.clone();
        if cfg.db.url.trim_end_matches('/') == crate::dbupdate::OLD_DEFAULT_URL {
            cfg.db.url = crate::dbupdate::DEFAULT_URL.into();
        }
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut c = self.clone();
        c.scanner.hotkey = c.hotkeys.scan.clone();
        let text = serde_json::to_string_pretty(&c).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    pub fn has_nation(&self, n: &str) -> bool {
        self.filters.nations.iter().any(|x| x == n)
    }

    pub fn has_type(&self, t: &str) -> bool {
        self.filters.types.iter().any(|x| x == t)
    }

    pub fn set_nation(&mut self, n: &str, on: bool) {
        let mut set: Vec<String> = NATIONS
            .iter()
            .map(|k| k.to_string())
            .filter(|k| if k == n { on } else { self.has_nation(k) })
            .collect();
        set.extend(self.filters.nations.iter().filter(|k| !NATIONS.contains(&k.as_str()) && *k != n).cloned());
        self.filters.nations = set;
    }

    pub fn set_type(&mut self, t: &str, on: bool) {
        self.filters.types = TYPES
            .iter()
            .map(|k| k.to_string())
            .filter(|k| if k == t { on } else { self.has_type(k) })
            .collect();
    }
}

fn apply_legacy(cfg: &mut Config, v: &Value) {
    let int = |k: &str| v.get(k).and_then(Value::as_i64);
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let b = |k: &str| v.get(k).and_then(Value::as_bool);
    let list = |k: &str| {
        v.get(k).and_then(Value::as_array).map(|a| {
            a.iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>()
        })
    };
    if let Some(x) = int("monitor_idx") { cfg.overlay.monitor_idx = x as i32; }
    if let Some(x) = int("x") { cfg.overlay.x = x as i32; }
    if let Some(x) = int("y") { cfg.overlay.y = x as i32; }
    if let Some(x) = int("size") { cfg.overlay.size = x as u32; }
    if let Some(x) = int("opa") { cfg.overlay.opacity = x as u32; }
    if let Some(x) = list("nations") { cfg.filters.nations = x; }
    if let Some(x) = list("types") { cfg.filters.types = x; }
    if let Some(r) = v.get("scan_region").and_then(|r| serde_json::from_value::<[i32; 4]>(r.clone()).ok()) {
        cfg.scanner.region = Some(r);
    }
    if let Some(x) = s("scan_hotkey") { cfg.hotkeys.scan = x; }
    if let Some(x) = s("overlay_hotkey") { cfg.hotkeys.overlay_toggle = x; }
    if let Some(x) = b("show_notes") { cfg.display.show_notes = x; }
    if let Some(x) = b("show_names") { cfg.display.show_names = x; }
    if let Some(x) = b("sound_enabled") { cfg.audio.enabled = x; }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_python_era_config() {
        let cfg = Config::parse(
            r#"{"overlay": {"x": 0, "y": 382, "size": 21, "opacity": 86, "monitor_idx": 1},
                "filters": {"nations": ["USA","Germany","USSR","Great Britain","Japan","China","Italy","France","Sweden","Israel"],
                            "types": ["tank","plane","heli","zsu","special"]},
                "scanner": {"region": [1289, 261, 472, 35], "confidence_threshold": 0.7, "hotkey": "\\"},
                "hotkeys": {"scan": "\\", "overlay_toggle": "f5"},
                "display": {"show_notes": false, "show_names": false},
                "audio": {"enabled": true}}"#,
        )
        .unwrap();
        assert_eq!(cfg.overlay.y, 382);
        assert_eq!(cfg.scanner.region, Some([1289, 261, 472, 35]));
        assert_eq!(cfg.hotkeys.scan, "\\");
        assert_eq!(cfg.hotkeys.overlay_toggle, "f5");
        assert!(!cfg.display.show_notes);
        assert_eq!(cfg.filters.nations.len(), 10);
    }

    #[test]
    fn parses_legacy_flat_config() {
        let cfg = Config::parse(
            r#"{"x": 5, "y": 6, "opa": 50, "nations": ["USA"], "scan_region": [1,2,3,4],
                "scan_hotkey": "f9", "sound_enabled": false}"#,
        )
        .unwrap();
        assert_eq!((cfg.overlay.x, cfg.overlay.y, cfg.overlay.opacity), (5, 6, 50));
        assert_eq!(cfg.filters.nations, vec!["USA"]);
        assert_eq!(cfg.scanner.region, Some([1, 2, 3, 4]));
        assert_eq!(cfg.hotkeys.scan, "f9");
        assert!(!cfg.audio.enabled);
    }

    #[test]
    fn old_db_url_is_migrated() {
        let cfg = Config::parse(&format!(r#"{{"db": {{"url": "{}"}}}}"#, crate::dbupdate::OLD_DEFAULT_URL)).unwrap();
        assert_eq!(cfg.db.url, crate::dbupdate::DEFAULT_URL);
        let cfg = Config::parse(r#"{"db": {"url": "https://example.org/db"}}"#).unwrap();
        assert_eq!(cfg.db.url, "https://example.org/db");
    }

    #[test]
    fn set_nation_keeps_order() {
        let mut cfg = Config::default();
        cfg.set_nation("Israel", true);
        cfg.set_nation("USA", true);
        assert_eq!(cfg.filters.nations, vec!["USA", "Israel"]);
        cfg.set_nation("USA", false);
        assert_eq!(cfg.filters.nations, vec!["Israel"]);
    }
}
