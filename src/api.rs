use crate::logic::HudMsg;
use crate::paths;
use crate::record::Recorder;
use crate::state::Ctl;
use serde_json::Value;
use std::time::Duration;

pub const API_URL: &str = "http://localhost:8111";

pub fn spawn(ctl: Ctl) {
    std::thread::Builder::new()
        .name("api-poller".into())
        .spawn(move || {
            let base = std::env::var("WT_API_URL").unwrap_or_else(|_| API_URL.to_string());
            let agent = ureq::AgentBuilder::new().timeout(Duration::from_millis(500)).build();
            let mut rec = Recorder::new(paths::file(paths::RECORDINGS_DIR));
            loop {
                let status = fetch_mission(&agent, &base);
                let map_loaded = status.is_some() && fetch_map_valid(&agent, &base);
                let (cursor, record) = ctl.with(|s| {
                    s.logic.on_mission(status.as_deref(), map_loaded);
                    let nick = s.cfg.player.nickname.clone();
                    s.logic.set_nickname(&nick);
                    (s.logic.hud_cursor(), s.cfg.record.enabled)
                });
                rec.tick(record, cursor.is_some());
                if rec.active() {
                    if let Some((vtype, army)) = fetch_indicators(&agent, &base) {
                        rec.own_vehicle(&vtype, &army);
                    }
                }
                if let Some(cursor) = cursor {
                    if let Some(msgs) = fetch_hud(&agent, &base, cursor) {
                        rec.messages(&msgs);
                        ctl.with(|s| {
                            let selected = s.cfg.filters.nations.iter().cloned().collect();
                            s.logic.on_damage(&msgs, &selected);
                        });
                    }
                }
                ctl.flush_sounds();
                ctl.repaint();
                std::thread::sleep(Duration::from_secs(1));
            }
        })
        .expect("api thread");
}

fn fetch_mission(agent: &ureq::Agent, base: &str) -> Option<String> {
    let v: Value = agent.get(&format!("{base}/mission.json")).call().ok()?.into_json().ok()?;
    Some(v.get("status").and_then(Value::as_str).unwrap_or("").to_string())
}

fn fetch_map_valid(agent: &ureq::Agent, base: &str) -> bool {
    agent
        .get(&format!("{base}/map_info.json"))
        .call()
        .ok()
        .and_then(|r| r.into_json::<Value>().ok())
        .and_then(|v| v.get("valid").and_then(Value::as_bool))
        .unwrap_or(false)
}

fn fetch_indicators(agent: &ureq::Agent, base: &str) -> Option<(String, String)> {
    let v: Value = agent.get(&format!("{base}/indicators")).call().ok()?.into_json().ok()?;
    if !v.get("valid").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    Some((s("type"), s("army")))
}

fn fetch_hud(agent: &ureq::Agent, base: &str, last_dmg: i64) -> Option<Vec<HudMsg>> {
    let v: Value = agent
        .get(&format!("{base}/hudmsg?lastEvt=-1&lastDmg={last_dmg}"))
        .call()
        .ok()?
        .into_json()
        .ok()?;
    let arr = v.get("damage").and_then(Value::as_array).cloned().unwrap_or_default();
    Some(
        arr.iter()
            .map(|m| HudMsg {
                id: m.get("id").and_then(Value::as_i64).unwrap_or(0),
                msg: m.get("msg").and_then(Value::as_str).unwrap_or("").to_string(),
            })
            .collect(),
    )
}
