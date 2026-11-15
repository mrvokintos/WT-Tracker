// UI translations: lang/<code>.json, embedded at build time. A <data dir>/lang/<code>.json
// overrides or adds a language without rebuilding. Missing keys fall back to English.

use crate::paths;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

const EMBEDDED: &[(&str, &str)] = &[
    ("en", include_str!("../lang/en.json")),
    ("ru", include_str!("../lang/ru.json")),
];

pub struct Language {
    pub code: String,
    pub name: String,
    strings: HashMap<String, String>,
}

static LANGS: OnceLock<Vec<Language>> = OnceLock::new();
static CURRENT: RwLock<usize> = RwLock::new(0);

fn parse(code: &str, text: &str) -> Option<Language> {
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let strings: HashMap<String, String> = v
        .as_object()?
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect();
    let name = strings.get("_name").cloned().unwrap_or_else(|| code.to_string());
    Some(Language { code: code.to_string(), name, strings })
}

fn load() -> Vec<Language> {
    let mut langs: Vec<Language> = EMBEDDED.iter().filter_map(|(c, t)| parse(c, t)).collect();
    if let Ok(rd) = std::fs::read_dir(paths::file("lang")) {
        for p in rd.flatten().map(|e| e.path()) {
            if p.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let code = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            match std::fs::read_to_string(&p).ok().and_then(|t| parse(&code, &t)) {
                Some(l) => merge(&mut langs, l),
                None => log::warn!("bad translation file {}", p.display()),
            }
        }
    }
    langs
}

/// A file only overrides the keys it has: an outdated ru.json left from an older release
/// must not turn new strings English. Lookup order: file -> embedded same language -> English.
fn merge(langs: &mut Vec<Language>, extra: Language) {
    match langs.iter_mut().find(|x| x.code == extra.code) {
        Some(existing) => existing.strings.extend(extra.strings),
        None => langs.push(extra),
    }
}

pub fn languages() -> &'static [Language] {
    LANGS.get_or_init(load)
}

/// "auto" picks the system language if there is a translation for it.
pub fn set(code: &str) {
    let code = if code == "auto" { system_language() } else { code.to_string() };
    let idx = languages().iter().position(|l| l.code == code).unwrap_or(0);
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = idx;
}

pub fn current_code() -> String {
    let i = *CURRENT.read().unwrap_or_else(|e| e.into_inner());
    languages()[i].code.clone()
}

fn system_language() -> String {
    sys_locale::get_locale()
        .and_then(|l| l.split(['-', '_']).next().map(str::to_lowercase))
        .filter(|c| languages().iter().any(|l| &l.code == c))
        .unwrap_or_else(|| "en".into())
}

pub fn t(key: &str) -> String {
    let langs = languages();
    let i = *CURRENT.read().unwrap_or_else(|e| e.into_inner());
    langs[i]
        .strings
        .get(key)
        .or_else(|| langs[0].strings.get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

pub fn tf(key: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut s = t(key);
    for (name, val) in args {
        s = s.replace(&format!("{{{name}}}"), &val.to_string());
    }
    s
}

#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::i18n::t($key)
    };
    ($key:literal, $($name:ident = $val:expr),+ $(,)?) => {
        $crate::i18n::tf($key, &[$((stringify!($name), &$val as &dyn std::fmt::Display)),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(s: &str) -> Vec<String> {
        let mut v: Vec<String> = s.split('{').skip(1).filter_map(|p| p.split_once('}').map(|x| x.0.to_string())).collect();
        v.sort();
        v
    }

    #[test]
    fn outdated_override_keeps_new_keys_translated() {
        let mut langs: Vec<Language> = EMBEDDED.iter().filter_map(|(c, t)| parse(c, t)).collect();
        let old = parse("ru", r#"{"db.update": "Обновить сейчас"}"#).unwrap();
        merge(&mut langs, old);
        let ru = langs.iter().find(|l| l.code == "ru").unwrap();
        assert_eq!(ru.strings["db.update"], "Обновить сейчас");
        assert_eq!(ru.strings["tab.settings"], "⚙ Настройки");
        let de = parse("de", r#"{"_name": "Deutsch", "db.update": "Aktualisieren"}"#).unwrap();
        merge(&mut langs, de);
        assert!(langs.iter().any(|l| l.code == "de"));
    }

    #[test]
    fn translations_complete() {
        let en = parse("en", EMBEDDED[0].1).unwrap();
        for (code, text) in &EMBEDDED[1..] {
            let l = parse(code, text).expect("valid json");
            for (k, v) in &en.strings {
                let tr = l.strings.get(k).unwrap_or_else(|| panic!("{code}: missing {k}"));
                assert_eq!(placeholders(v), placeholders(tr), "{code}: placeholders differ in {k}");
            }
            for k in l.strings.keys() {
                assert!(en.strings.contains_key(k), "{code}: unknown key {k}");
            }
        }
    }
}
