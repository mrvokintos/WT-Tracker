use crate::logic::HudMsg;
use serde_json::json;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Recorder {
    dir: PathBuf,
    file: Option<File>,
    max_id: i64,
    last_own: String,
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

impl Recorder {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, file: None, max_id: 0, last_own: String::new() }
    }

    fn write(&mut self, v: serde_json::Value) {
        if let Some(f) = &mut self.file {
            let _ = writeln!(f, "{v}");
        }
    }

    pub fn tick(&mut self, enabled: bool, in_battle: bool) {
        let want = enabled && in_battle;
        if want && self.file.is_none() {
            let _ = std::fs::create_dir_all(&self.dir);
            let path = self.dir.join(format!("battle_{}.jsonl", now_ms() / 1000));
            match File::create(&path) {
                Ok(f) => {
                    log::info!("Запись боя: {}", path.display());
                    // hudmsg ids don't reset between battles
                    self.file = Some(f);
                    self.last_own.clear();
                    self.write(json!({"t": now_ms(), "kind": "start"}));
                }
                Err(e) => log::error!("Не удалось создать {}: {e}", path.display()),
            }
        } else if !want && self.file.is_some() {
            self.write(json!({"t": now_ms(), "kind": "end"}));
            self.file = None;
        }
    }

    pub fn active(&self) -> bool {
        self.file.is_some()
    }

    pub fn messages(&mut self, msgs: &[HudMsg]) {
        // ids went backwards = game restarted
        if msgs.last().is_some_and(|m| m.id < self.max_id) {
            self.max_id = 0;
        }
        let new: Vec<_> = msgs.iter().filter(|m| m.id > self.max_id).cloned().collect();
        for m in new {
            self.max_id = self.max_id.max(m.id);
            self.write(json!({"t": now_ms(), "kind": "dmg", "id": m.id, "msg": m.msg}));
        }
    }

    pub fn own_vehicle(&mut self, vtype: &str, army: &str) {
        // dummy_plane = respawn screen
        if !vtype.is_empty() && !vtype.ends_with("dummy_plane") && vtype != self.last_own {
            self.last_own = vtype.to_string();
            self.write(json!({"t": now_ms(), "kind": "own", "type": vtype, "army": army}));
        }
    }
}
