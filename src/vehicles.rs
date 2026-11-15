use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct VehicleInfo {
    #[serde(default = "unknown_nation")]
    pub nation: String,
    #[serde(default = "special_type", rename = "type")]
    pub vtype: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub nations: Vec<String>,
    #[serde(default)]
    pub id: Option<String>,
    /// all vehicles sharing this name (MQ-1 in several trees); empty when it's just `id`
    #[serde(default)]
    pub ids: Vec<String>,
    /// flag shown on the scoreboard (operator country), e.g. "russia" for a USSR-tree T-90M
    #[serde(default)]
    pub operator: Option<String>,
    #[serde(default)]
    pub operators: Vec<String>,
}

/// 48- and 50-star US flags can't be told apart on the scoreboard; treat them as one flag.
pub fn canon_flag(code: &str) -> String {
    let c = flag_code(code);
    if c == "usa_modern" { "usa".into() } else { c }
}

impl VehicleInfo {
    /// Scoreboard flags of the vehicle (canonical codes); empty for an old database.
    pub fn operator_list(&self) -> Vec<String> {
        let raw: Vec<&String> = if self.operators.is_empty() { self.operator.iter().collect() } else { self.operators.iter().collect() };
        let mut out: Vec<String> = vec![];
        for o in raw {
            let c = canon_flag(o);
            if !out.contains(&c) {
                out.push(c);
            }
        }
        out
    }

    pub fn nation_list(&self) -> Vec<String> {
        if self.nations.is_empty() {
            vec![self.nation.clone()]
        } else {
            self.nations.clone()
        }
    }
}

fn unknown_nation() -> String {
    "unknown".into()
}
fn special_type() -> String {
    "special".into()
}

#[derive(Default)]
pub struct VehicleDb {
    exact: HashMap<String, VehicleInfo>,
    lower: HashMap<String, String>,
    /// scoreboard flag code -> tree nations, most common first
    flags: HashMap<String, Vec<String>>,
    /// unique vehicles (by game id) per type; the json has one entry per name and language
    types: std::collections::BTreeMap<String, usize>,
}

/// Tree flags; also the fallback for a database without operator data.
pub const TREE_FLAGS: &[(&str, &str)] = &[
    ("usa", "USA"),
    ("germany", "Germany"),
    ("ussr", "USSR"),
    ("britain", "Great Britain"),
    ("japan", "Japan"),
    ("china", "China"),
    ("italy", "Italy"),
    ("france", "France"),
    ("sweden", "Sweden"),
    ("israel", "Israel"),
];

/// Team flags of event battles that no vehicle carries (the Russian Empire is in the USSR tree).
const EVENT_FLAGS: &[(&str, &[&str])] = &[
    ("triple_entente", &["Great Britain", "France", "USSR"]),
];

/// Template file name -> flag code (also accepts the old "Great Britain.png" style names).
pub fn flag_code(name: &str) -> String {
    let n = name.trim().to_lowercase().replace(' ', "_");
    if n == "great_britain" { "britain".into() } else { n }
}

/// Killstreak aircraft and drones aren't player vehicles and never show up on the scoreboard.
fn is_killstreak(id: &str) -> bool {
    id.contains("killstreak") || id.starts_with("ucav_")
}

/// Scoreboard flag -> every tree that has a player vehicle with that flag, most vehicles first.
/// One vehicle is enough: the M1A1 AIM puts the Australian flag in the US tree.
fn flag_map(exact: &HashMap<String, VehicleInfo>) -> HashMap<String, Vec<String>> {
    let mut seen: HashMap<String, HashMap<String, BTreeSet<&str>>> = HashMap::new();
    for v in exact.values() {
        // names shared by several vehicles mix up their flags and trees, the other names cover them
        let (Some(id), Some(op)) = (v.id.as_deref(), v.operator.as_deref()) else { continue };
        if v.ids.len() > 1 || !v.operators.is_empty() || v.nation == "unknown" || is_killstreak(id) {
            continue;
        }
        seen.entry(flag_code(op)).or_default().entry(v.nation.clone()).or_default().insert(id);
    }
    let mut out: HashMap<String, Vec<String>> = seen
        .into_iter()
        .map(|(code, trees)| {
            let mut trees: Vec<(String, usize)> = trees.into_iter().map(|(n, ids)| (n, ids.len())).collect();
            trees.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            (code, trees.into_iter().map(|t| t.0).collect())
        })
        .collect();
    for (code, nation) in TREE_FLAGS {
        out.entry(code.to_string()).or_insert_with(|| vec![nation.to_string()]);
    }
    for (code, trees) in EVENT_FLAGS {
        out.entry(code.to_string()).or_insert_with(|| trees.iter().map(|t| t.to_string()).collect());
    }
    out
}

pub fn normalize(name: &str) -> String {
    name.replace('\u{a0}', " ").replace('\u{200b}', "").trim().to_lowercase()
}

impl VehicleDb {
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        let exact: HashMap<String, VehicleInfo> =
            serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
        let mut lower = HashMap::with_capacity(exact.len());
        let mut keys: Vec<&String> = exact.keys().collect();
        keys.sort();
        for k in keys {
            lower.entry(normalize(k)).or_insert_with(|| k.clone());
        }
        let flags = flag_map(&exact);
        let mut ids: HashMap<&str, &str> = HashMap::new();
        for v in exact.values() {
            for id in v.ids.iter().map(String::as_str).chain(v.id.as_deref()) {
                ids.insert(id, &v.vtype);
            }
        }
        let mut types = std::collections::BTreeMap::new();
        for t in ids.values() {
            *types.entry(t.to_string()).or_insert(0) += 1;
        }
        Ok(Self { exact, lower, flags, types })
    }

    pub fn load(path: &Path, embedded: &str) -> Self {
        if let Ok(text) = std::fs::read_to_string(path) {
            match Self::from_json(&text) {
                Ok(db) => {
                    log::info!("Загружено {} ед. техники ({} названий) из {}", db.vehicle_count(), db.len(), path.display());
                    return db;
                }
                Err(e) => log::error!("Ошибка чтения {}: {e}; использую встроенную базу", path.display()),
            }
        }
        let db = Self::from_json(embedded).unwrap_or_default();
        log::info!("Загружено {} ед. техники ({} названий, встроенная база)", db.vehicle_count(), db.len());
        db
    }

    /// Number of names (json entries), what the manifest counts.
    pub fn len(&self) -> usize {
        self.exact.len()
    }

    /// Unique vehicles by game id.
    pub fn vehicle_count(&self) -> usize {
        self.types.values().sum()
    }

    pub fn type_counts(&self) -> &std::collections::BTreeMap<String, usize> {
        &self.types
    }

    /// Tree nations for a scoreboard flag ("russia" -> USSR, "australia" -> Great Britain).
    pub fn flag_nations(&self, flag: &str) -> Vec<String> {
        self.flags.get(&flag_code(flag)).cloned().unwrap_or_default()
    }

    /// Wiki-based database from the Python version: no game ids, names often don't match the kill feed.
    pub fn is_legacy(&self) -> bool {
        !self.exact.is_empty() && self.exact.values().all(|v| v.id.is_none())
    }

    pub fn get(&self, name: &str) -> Option<&VehicleInfo> {
        self.exact
            .get(name)
            .or_else(|| self.lower.get(&normalize(name)).and_then(|k| self.exact.get(k)))
    }
}

pub fn save_unknowns(path: &Path, new: &BTreeSet<String>) -> std::io::Result<usize> {
    if new.is_empty() {
        return Ok(0);
    }
    let mut all: BTreeSet<String> = std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .map(str::to_string)
        .collect();
    all.extend(new.iter().cloned());
    let mut out = String::from(
        "// Vehicle names from the kill feed that are missing in vehicles.json\n\
         // Format: \"Name\": {\"type\": \"tank\", \"nation\": \"USA\", \"note\": \"...\"}\n\n",
    );
    for n in &all {
        out.push_str(n);
        out.push('\n');
    }
    std::fs::write(path, out)?;
    Ok(all.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_db_loads() {
        let db = VehicleDb::from_json(crate::paths::EMBEDDED_VEHICLES).unwrap();
        assert!(db.len() > 2000);
        // thousands of names, but each vehicle is counted once
        let n = db.vehicle_count();
        assert!((2500..db.len() / 2).contains(&n), "{n}");
        assert!(db.type_counts()["tank"] > 900 && db.type_counts()["plane"] > 1000, "{:?}", db.type_counts());
        assert_eq!(db.get("CM11").unwrap().nation, "China");
        assert_eq!(db.get("  cm11 ").unwrap().vtype, "tank");
        assert_eq!(db.get("M1A1 SA").unwrap().nation, "USA");
        assert!(db.get("MQ-1").unwrap().nations.len() > 1);
        assert!(!db.is_legacy());
        assert_eq!(db.flag_nations("russia"), vec!["USSR"]);
        assert_eq!(db.flag_nations("germany_modern"), vec!["Germany"]);
        assert_eq!(db.flag_nations("republic_china"), vec!["China"]);
        assert_eq!(db.flag_nations("australia"), vec!["Great Britain", "USA"], "M1A1 AIM is in the US tree");
        assert_eq!(db.flag_nations("canada"), vec!["Great Britain", "USA"]);
        assert_eq!(db.flag_nations("ussr"), vec!["USSR"], "killstreaks and shared names don't count");
        assert_eq!(db.flag_nations("usa"), vec!["USA"]);
        assert_eq!(db.flag_nations("triple_entente"), vec!["Great Britain", "France", "USSR"]);
        assert_eq!(db.flag_nations("germany_empire"), vec!["Germany"]);
        assert_eq!(db.flag_nations("Great Britain"), db.flag_nations("britain"), "old template names");
        assert_eq!(db.flag_nations("britain")[0], "Great Britain");
        assert!(db.flag_nations("atlantis").is_empty());
        assert!(VehicleDb::from_json(r#"{"M1A2 Abrams": {"nation": "USA", "type": "tank"}}"#).unwrap().is_legacy());
    }
}
