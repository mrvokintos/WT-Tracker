use crate::config::Config;
use crate::killfeed::{self, Event};
use crate::vehicles::{canon_flag, VehicleDb, VehicleInfo};
use regex::Regex;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

pub const NUKE_KILLS_THRESHOLD: u32 = 9;
pub const NUKE_DEATHS_MAX: u32 = 2;
// late kill messages from someone who just died in that vehicle
pub const RESPAWN_IGNORE: Duration = Duration::from_secs(25);
pub const LEAVE_POLLS: u32 = 3;

const SHARED_VEHICLES: &[&str] =
    &["mq-1", "▄m55", "clovis", "df105", "m44", "▄m44", "nasams 3 (соу)", "nasams 3 (соц)"];

const GARBAGE_PATTERNS: &[&str] = &[
    "weapons/", "us_hellfire ", "agm_", "tow ", "short ", "aim_", "mim_", "kh_", "rocket", "bomb ",
    "torpedo ", "su_9m", "m8_hvap", "m82_shot", "su_r_73", "us_iris_t_sl", "fb10_",
];

static RE_ENTITY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(.+?)\s\((.+)\)$").unwrap());
static RE_TIME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+:\d+").unwrap());
static RE_AI_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\[(ии|ai)\]\s*").unwrap());
static RE_CALIBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+mm_").unwrap());
static RE_9M: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^9m\d+").unwrap());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundKind {
    Enemy,
    Ally,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerStats {
    pub kills: u32,
    pub deaths: u32,
    pub disqualified: bool,
    pub alert_played: bool,
}

impl PlayerStats {
    pub fn can_nuke(&self) -> bool {
        self.kills >= NUKE_KILLS_THRESHOLD && self.deaths <= NUKE_DEATHS_MAX && !self.disqualified
    }
}

/// A living player seen in this battle, whatever the side.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivePlayer {
    pub vehicle: String,
    pub info: Option<VehicleInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Enemy,
    Ally,
}

impl Side {
    fn opposite(self) -> Side {
        match self {
            Side::Enemy => Side::Ally,
            Side::Ally => Side::Enemy,
        }
    }
}

/// Why a side was assigned; a higher rank is never overwritten by a lower one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    Graph = 1,
    Seed = 2,
    Me = 3,
}

/// Both teams from a scoreboard scan: operator flags and the tech trees they map to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TeamContext {
    pub enemy_flags: HashSet<String>,
    pub own_flags: HashSet<String>,
    pub enemy_trees: HashSet<String>,
    pub own_trees: HashSet<String>,
}

#[derive(Debug, Clone)]
struct KillEdge {
    killer: String,
    victim: String,
}

#[derive(Debug, Clone)]
struct Death {
    vehicle: String,
    at: Instant,
}

#[derive(Debug, Clone)]
pub struct HudMsg {
    pub id: i64,
    pub msg: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverlayLine {
    pub text: String,
    pub nuke: bool,
}

pub struct Logic {
    db: VehicleDb,
    pub api_available: bool,
    pub in_battle: bool,
    skip_history: bool,
    last_dmg_id: i64,
    not_in_battle_polls: u32,
    /// living players in insertion (display) order
    pub active: Vec<(String, ActivePlayer)>,
    dead: HashMap<String, Death>,
    pub players: HashMap<String, PlayerStats>,
    cache: HashMap<String, Option<VehicleInfo>>,
    pub unknown: BTreeSet<String>,
    sounds: Vec<SoundKind>,
    // side detection, all reset with the battle
    teams: Option<TeamContext>,
    edges: Vec<KillEdge>,
    /// every vehicle a player was seen in this battle
    vehicles: HashMap<String, Vec<(String, Option<VehicleInfo>)>>,
    sides: HashMap<String, (Side, Rank)>,
    pub side_conflicts: u32,
    selected: HashSet<String>,
    me: String,
}

fn parse_entity(text: &str) -> Option<(String, String)> {
    let t = text.trim();
    let c = RE_ENTITY.captures(t)?;
    let player = c[1].trim().to_string();
    let veh = c[2].trim().to_string();
    // ai sams show up as "CLAWS (СОУ)" with no player
    let up = veh.to_uppercase();
    if up == "СОУ" || up == "СОЦ" {
        return Some(("AI_System".into(), t.to_string()));
    }
    Some((player, veh))
}

fn strip_time(name: &str) -> String {
    RE_TIME.replace(name, "").trim().to_string()
}

fn strip_ai(veh: &str) -> String {
    RE_AI_TAG.replace_all(veh, "").trim().to_string()
}

fn is_garbage(name: &str) -> bool {
    let l = name.to_lowercase();
    RE_CALIBER.is_match(&l)
        || RE_9M.is_match(&l)
        || l.contains("recon micro")
        || l.contains('/')
        || GARBAGE_PATTERNS.iter().any(|p| l.contains(p))
}

pub fn is_air(vtype: &str) -> bool {
    matches!(vtype, "plane" | "heli" | "jet_fighter")
}

pub fn type_key(vtype: &str) -> &'static str {
    let t = vtype.to_lowercase();
    if t.contains("tank") || t.contains("mbt") {
        "tank"
    } else if t.contains("plane") || t.contains("fighter") {
        "plane"
    } else if t.contains("heli") {
        "heli"
    } else if t.contains("spaa") || t.contains("zsu") {
        "zsu"
    } else if t.contains("aircraft") {
        "plane"
    } else {
        "special"
    }
}

pub fn type_icon(vtype: &str) -> &'static str {
    match vtype {
        "tank" => "🛡 ",
        "plane" => "✈ ",
        "heli" => "🚁 ",
        "zsu" => "📡 ",
        "special" => "⭐ ",
        _ => "• ",
    }
}

pub fn is_visible(cfg: &Config, nations: &[String], vtype: &str) -> bool {
    if nations.is_empty() || nations.iter().all(|n| n == "unknown") {
        return true;
    }
    nations.iter().any(|n| cfg.has_nation(n)) && cfg.has_type(type_key(vtype))
}

impl Logic {
    pub fn new(db: VehicleDb) -> Self {
        Self {
            db,
            api_available: false,
            in_battle: false,
            skip_history: true,
            last_dmg_id: 0,
            not_in_battle_polls: 0,
            active: Vec::new(),
            dead: HashMap::new(),
            players: HashMap::new(),
            cache: HashMap::new(),
            unknown: BTreeSet::new(),
            sounds: Vec::new(),
            teams: None,
            edges: Vec::new(),
            vehicles: HashMap::new(),
            sides: HashMap::new(),
            side_conflicts: 0,
            selected: HashSet::new(),
            me: String::new(),
        }
    }

    pub fn db_counts(&self) -> (usize, &std::collections::BTreeMap<String, usize>) {
        (self.db.vehicle_count(), self.db.type_counts())
    }

    /// Tree nations for scoreboard flags ("russia" -> USSR), without duplicates.
    pub fn flags_to_nations(&self, flags: &[String]) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for f in flags {
            for n in self.db.flag_nations(f) {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
        }
        out
    }

    pub fn replace_db(&mut self, db: VehicleDb) {
        self.db = db;
        self.cache.clear();
    }

    pub fn take_sounds(&mut self) -> Vec<SoundKind> {
        std::mem::take(&mut self.sounds)
    }

    pub fn take_unknowns(&mut self) -> BTreeSet<String> {
        std::mem::take(&mut self.unknown)
    }

    fn reset(&mut self) {
        self.active.clear();
        self.dead.clear();
        self.players.clear();
        self.cache.clear();
        self.teams = None;
        self.edges.clear();
        self.vehicles.clear();
        self.sides.clear();
        self.side_conflicts = 0;
        self.skip_history = true;
        log::info!(">>> НОВАЯ СЕССИЯ: данные сброшены");
    }

    // mission.json keeps saying "running" in the hangar after a battle,
    // so a loaded map is required too
    pub fn on_mission(&mut self, status: Option<&str>, map_loaded: bool) {
        self.api_available = status.is_some();
        let now = status == Some("running") && map_loaded;
        if now {
            self.not_in_battle_polls = 0;
            if !self.in_battle {
                self.reset();
                self.in_battle = true;
            }
        } else if self.in_battle {
            self.not_in_battle_polls += 1;
            if self.not_in_battle_polls >= LEAVE_POLLS {
                self.not_in_battle_polls = 0;
                self.in_battle = false;
                self.reset();
            }
        }
    }

    pub fn hud_cursor(&self) -> Option<i64> {
        (self.api_available && self.in_battle)
            .then_some(if self.skip_history { 0 } else { self.last_dmg_id })
    }

    pub fn on_damage(&mut self, msgs: &[HudMsg], selected: &HashSet<String>) {
        if !self.in_battle {
            return;
        }
        if *selected != self.selected {
            self.selected = selected.clone();
            self.recompute_sides();
        }
        let Some(last) = msgs.last() else {
            self.skip_history = false;
            return;
        };
        // first request returns old messages, skip them
        if self.skip_history {
            self.last_dmg_id = last.id;
            self.skip_history = false;
            return;
        }
        let mut max_id = self.last_dmg_id;
        for m in msgs {
            if m.id <= self.last_dmg_id {
                continue;
            }
            match killfeed::parse(&m.msg) {
                Some(Event::Kill { killer, victim }) => self.process_kill(&killer, &victim),
                Some(Event::Death { who }) => self.process_death(&who),
                None => {}
            }
            max_id = max_id.max(m.id);
        }
        self.last_dmg_id = max_id;
    }

    fn find_vehicle(&mut self, log_name: &str) -> Option<VehicleInfo> {
        if log_name.is_empty() {
            return None;
        }
        if let Some(c) = self.cache.get(log_name) {
            return c.clone();
        }
        let res = if is_garbage(log_name) {
            None
        } else {
            let l = log_name.to_lowercase();
            if SHARED_VEHICLES.iter().any(|s| l.contains(s)) {
                let mq = l.contains("mq-1");
                Some(VehicleInfo {
                    nation: "unknown".into(),
                    vtype: if mq { "aircraft" } else { "tank" }.into(),
                    note: if mq { crate::t!("note.drone") } else { crate::t!("note.clone") },
                    nations: vec![],
                    id: None,
                    ids: vec![],
                    operator: None,
                    operators: vec![],
                })
            } else if let Some(info) = self.db.get(log_name) {
                Some(info.clone())
            } else {
                self.unknown.insert(log_name.to_string());
                None
            }
        };
        self.cache.insert(log_name.to_string(), res.clone());
        res
    }

    fn player(&mut self, name: &str) -> &mut PlayerStats {
        self.players.entry(name.to_string()).or_default()
    }

    fn process_kill(&mut self, killer_raw: &str, victim_raw: &str) {
        let Some((killer_name, killer_veh)) = parse_entity(killer_raw) else {
            return;
        };
        let (victim_name, victim_veh) = match parse_entity(victim_raw) {
            Some((n, v)) => (strip_time(&n), v),
            None => ("AI_Drone".to_string(), victim_raw.trim().to_string()),
        };
        let killer_name = strip_time(&killer_name);
        let killer_clean = strip_ai(&killer_veh);
        let victim_clean = strip_ai(&victim_veh);

        let killer_info = self.find_vehicle(&killer_clean);
        let victim_info = self.find_vehicle(&victim_clean);

        let air = killer_info.as_ref().is_some_and(|i| is_air(&i.vtype));
        let p = self.player(&killer_name);
        p.kills += 1;
        if air {
            p.disqualified = true;
        }
        self.add_active(&killer_name, &killer_veh, killer_info.clone());

        let recon = victim_clean.to_lowercase().contains("recon micro");
        if graph_player(&killer_name) {
            self.remember_vehicle(&killer_name, &killer_clean, killer_info);
            if graph_player(&victim_name) && !recon {
                self.remember_vehicle(&victim_name, &victim_clean, victim_info.clone());
                self.edges.push(KillEdge { killer: killer_name.clone(), victim: victim_name.clone() });
            }
            self.recompute_sides();
        }
        self.check_nuke(&killer_name);

        if !recon {
            self.process_victim(&victim_name, &victim_veh, victim_info.as_ref());
        }
    }

    fn process_death(&mut self, who: &str) {
        let Some((name, veh)) = parse_entity(who) else {
            return;
        };
        let name = strip_time(&name);
        let clean = strip_ai(&veh);
        let info = self.find_vehicle(&clean);
        if graph_player(&name) {
            self.remember_vehicle(&name, &clean, info.clone());
        }
        self.process_victim(&name, &veh, info.as_ref());
    }

    fn remember_vehicle(&mut self, name: &str, vehicle: &str, info: Option<VehicleInfo>) {
        let seen = self.vehicles.entry(name.to_string()).or_default();
        if !seen.iter().any(|(v, _)| v == vehicle) {
            seen.push((vehicle.to_string(), info));
        }
    }

    fn add_active(&mut self, name: &str, vehicle: &str, info: Option<VehicleInfo>) {
        if let Some(d) = self.dead.get(name) {
            if d.vehicle == vehicle && d.at.elapsed() < RESPAWN_IGNORE {
                return;
            }
            self.dead.remove(name);
        }
        let a = ActivePlayer { vehicle: vehicle.to_string(), info };
        match self.active.iter_mut().find(|(n, _)| n == name) {
            Some((_, e)) => *e = a,
            None => self.active.push((name.to_string(), a)),
        }
    }

    fn process_victim(&mut self, name: &str, vehicle: &str, info: Option<&VehicleInfo>) {
        let air = info.is_some_and(|i| is_air(&i.vtype));
        let p = self.player(name);
        p.deaths += 1;
        if air {
            p.disqualified = true;
        }
        let mut death_veh = vehicle.to_string();
        if let Some(pos) = self.active.iter().position(|(n, _)| n == name) {
            death_veh = self.active.remove(pos).1.vehicle;
        }
        self.dead.insert(name.to_string(), Death { vehicle: death_veh, at: Instant::now() });
    }

    fn check_nuke(&mut self, name: &str) {
        let side = self.side_of(name);
        let p = self.player(name);
        if p.can_nuke() && !p.alert_played {
            p.alert_played = true;
            // unknown side gets the enemy alert, a missed enemy nuke is worse than a false alarm
            self.sounds.push(if side == Some(Side::Ally) { SoundKind::Ally } else { SoundKind::Enemy });
        }
    }

    pub fn enemy_count(&self) -> usize {
        self.active.iter().filter(|(n, _)| self.side_of(n) == Some(Side::Enemy)).count()
    }

    pub fn side_of(&self, name: &str) -> Option<Side> {
        self.sides.get(name).map(|s| s.0)
    }

    #[cfg(test)]
    pub fn teams(&self) -> Option<&TeamContext> {
        self.teams.as_ref()
    }

    /// Builds the scan context; trees come from the operator-flag mapping of the database.
    pub fn team_context(&self, enemy_flags: &[String], own_flags: &[String]) -> TeamContext {
        TeamContext {
            enemy_flags: enemy_flags.iter().map(|f| canon_flag(f)).collect(),
            own_flags: own_flags.iter().map(|f| canon_flag(f)).collect(),
            enemy_trees: self.flags_to_nations(enemy_flags).into_iter().collect(),
            own_trees: self.flags_to_nations(own_flags).into_iter().collect(),
        }
    }

    pub fn set_teams(&mut self, ctx: TeamContext) {
        self.teams = Some(ctx);
        self.recompute_sides();
    }

    pub fn set_nickname(&mut self, nick: &str) {
        let nick = nick.trim();
        if nick != self.me {
            self.me = nick.to_string();
            self.recompute_sides();
        }
    }

    /// Two-colours the kill graph: every kill joins players of different teams.
    /// Starting points are the player (by nickname) and vehicles whose flag or tree is on one side only.
    fn recompute_sides(&mut self) {
        self.sides.clear();
        let mut names: Vec<String> = self.vehicles.keys().cloned().collect();
        names.sort();
        let mut queue: VecDeque<String> = VecDeque::new();
        for name in &names {
            if is_me(name, &self.me) {
                self.sides.insert(name.clone(), (Side::Ally, Rank::Me));
                queue.push_back(name.clone());
                continue;
            }
            let seeds: Vec<Side> = self.vehicles[name]
                .iter()
                .filter_map(|(_, info)| info.as_ref().and_then(|i| vehicle_seed(i, self.teams.as_ref(), &self.selected)))
                .collect();
            match seeds.first() {
                Some(&s) if seeds.iter().all(|&x| x == s) => {
                    self.sides.insert(name.clone(), (s, Rank::Seed));
                    queue.push_back(name.clone());
                }
                Some(_) => log::debug!("{name}: vehicles point to different sides, no seed"),
                None => {}
            }
        }
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        for e in &self.edges {
            adj.entry(&e.killer).or_default().push(&e.victim);
            adj.entry(&e.victim).or_default().push(&e.killer);
        }
        while let Some(p) = queue.pop_front() {
            let want = self.sides[&p].0.opposite();
            for &q in adj.get(p.as_str()).map(Vec::as_slice).unwrap_or(&[]) {
                if !self.sides.contains_key(q) {
                    self.sides.insert(q.to_string(), (want, Rank::Graph));
                    queue.push_back(q.to_string());
                }
            }
        }
        // a kill inside one team: teamkill or a wrong guess; existing sides stay
        self.side_conflicts = self
            .edges
            .iter()
            .filter(|e| match (self.sides.get(&e.killer), self.sides.get(&e.victim)) {
                (Some(a), Some(b)) => a.0 == b.0,
                _ => false,
            })
            .count() as u32;
        if self.side_conflicts > 0 {
            log::debug!("side conflicts: {}", self.side_conflicts);
        }
    }

    pub fn overlay_lines(&self, cfg: &Config) -> Vec<OverlayLine> {
        let selected: HashSet<String> = cfg.filters.nations.iter().cloned().collect();
        let mut out = Vec::new();
        for (name, a) in &self.active {
            let trees = known_nations(a.info.as_ref()).unwrap_or_default();
            let uncertain = match self.side_of(name) {
                Some(Side::Ally) => continue,
                Some(Side::Enemy) => false,
                None => {
                    // unresolved: only worth a "?" if the player's tree is on the enemy side (or unknown)
                    let enemy_trees = match &self.teams {
                        Some(t) => &t.enemy_trees,
                        None if selected.is_empty() => continue,
                        None => &selected,
                    };
                    if !trees.is_empty() && !trees.iter().any(|n| enemy_trees.contains(n)) {
                        continue;
                    }
                    true
                }
            };
            let vtype = a.info.as_ref().map_or("aircraft".to_string(), |i| i.vtype.to_lowercase());
            let nations = a.info.as_ref().map_or(vec!["unknown".to_string()], |i| i.nation_list());
            if !is_visible(cfg, &nations, &vtype) {
                continue;
            }
            let stats = self.players.get(name);
            let nuke = stats.is_some_and(PlayerStats::can_nuke);
            let mut text = if cfg.display.show_names || nuke {
                format!("{} ({})", a.vehicle, name)
            } else {
                a.vehicle.clone()
            };
            let note = a.info.as_ref().map_or("", |i| i.note.as_str());
            if cfg.display.show_notes && !note.is_empty() {
                text.push_str(" — ");
                text.push_str(note);
            }
            let text = if nuke {
                format!("☢ {} [{} K]", text, stats.map_or(0, |s| s.kills))
            } else if uncertain {
                format!("? {text}")
            } else {
                format!("{}{}", type_icon(&vtype), text)
            };
            out.push(OverlayLine { text, nuke });
        }
        out
    }
}

fn known_nations(info: Option<&VehicleInfo>) -> Option<Vec<String>> {
    let list: Vec<String> = info?.nation_list().into_iter().filter(|n| n != "unknown").collect();
    (!list.is_empty()).then_some(list)
}

/// AI and unnamed units don't belong to a player and don't join the graph.
fn graph_player(name: &str) -> bool {
    !name.is_empty() && name != "AI_Drone" && name != "AI_System"
}

/// The kill feed name may carry a clan tag: "[HARBS] mrvokintos". Nicknames are case-sensitive.
pub fn is_me(name: &str, me: &str) -> bool {
    !me.is_empty() && (name == me || name.ends_with(&format!(" {me}")))
}

/// Starting side from the vehicle alone. With a scan of both teams: a flag (then a tree) present
/// on one side only. Without it: the old rule, a selected tree is the enemy.
fn vehicle_seed(info: &VehicleInfo, teams: Option<&TeamContext>, selected: &HashSet<String>) -> Option<Side> {
    let trees = known_nations(Some(info)).unwrap_or_default();
    let Some(ctx) = teams else {
        if selected.is_empty() || trees.is_empty() {
            return None;
        }
        return Some(if trees.iter().any(|n| selected.contains(n)) { Side::Enemy } else { Side::Ally });
    };
    let one_side = |items: &[String], enemy: &HashSet<String>, own: &HashSet<String>| {
        if items.is_empty() {
            None
        } else if items.iter().all(|x| enemy.contains(x) && !own.contains(x)) {
            Some(Side::Enemy)
        } else if items.iter().all(|x| own.contains(x) && !enemy.contains(x)) {
            Some(Side::Ally)
        } else {
            None
        }
    };
    one_side(&info.operator_list(), &ctx.enemy_flags, &ctx.own_flags)
        .or_else(|| one_side(&trees, &ctx.enemy_trees, &ctx.own_trees))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DB: &str = r#"{
        "Leopard 2A7V": {"nation": "Germany", "type": "tank", "note": "ОБТ"},
        "Т-90М": {"nation": "USSR", "type": "tank"},
        "Ка-52": {"nation": "USSR", "type": "heli"},
        "Pantsir-S1": {"nation": "USSR", "type": "zsu"},
        "F-16C": {"nation": "USA", "type": "plane"}
    }"#;

    fn logic() -> Logic {
        let mut l = Logic::new(VehicleDb::from_json(DB).unwrap());
        l.on_mission(Some("running"), true);
        l.on_damage(&[], &sel(&[]));
        l
    }

    fn sel(n: &[&str]) -> HashSet<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    fn feed(l: &mut Logic, id: i64, msg: &str, s: &HashSet<String>) {
        l.on_damage(&[HudMsg { id, msg: msg.into() }], s);
    }

    /// living players currently resolved as enemies
    fn enemies(l: &Logic) -> Vec<&str> {
        l.active.iter().filter(|(n, _)| l.side_of(n) == Some(Side::Enemy)).map(|(n, _)| n.as_str()).collect()
    }

    #[test]
    fn parse_entity_cases() {
        assert_eq!(parse_entity("Bob (Т-90М)"), Some(("Bob".into(), "Т-90М".into())));
        assert_eq!(
            parse_entity("Bob (LVT(A)(4) (ZiS-2))"),
            Some(("Bob".into(), "LVT(A)(4) (ZiS-2)".into()))
        );
        assert_eq!(parse_entity("CLAWS (СОУ)"), Some(("AI_System".into(), "CLAWS (СОУ)".into())));
        assert_eq!(parse_entity("no vehicle"), None);
    }

    #[test]
    fn kill_marks_enemy_and_victim_death() {
        let mut l = logic();
        let s = sel(&["Germany"]);
        feed(&mut l, 1, "12:03 =TAG= Hans (Leopard 2A7V) уничтожил Ivan (Т-90М)", &s);
        assert_eq!(enemies(&l), vec!["=TAG= Hans"]);
        assert_eq!(l.players["=TAG= Hans"].kills, 1);
        assert_eq!(l.players["Ivan"].deaths, 1);
        feed(&mut l, 2, "Ivan (Т-90М) destroyed Hans2 (Leopard 2A7V)", &s);
        assert_eq!(enemies(&l), vec!["=TAG= Hans"]);
    }

    #[test]
    fn enemy_removed_on_death_and_solo() {
        let mut l = logic();
        let s = sel(&["Germany", "USA"]);
        feed(&mut l, 1, "Hans (Leopard 2A7V) уничтожил Ivan (Т-90М)", &s);
        feed(&mut l, 2, "Joe (F-16C) сбил Petr (Ка-52)", &s);
        assert_eq!(enemies(&l), vec!["Hans", "Joe"]);
        feed(&mut l, 3, "Ivan (Т-90М) уничтожил Hans (Leopard 2A7V)", &s);
        assert_eq!(enemies(&l), vec!["Joe"]);
        feed(&mut l, 4, "Joe (F-16C) разбился", &s);
        assert!(enemies(&l).is_empty());
        assert_eq!(l.players["Joe"].deaths, 1, "смерть считается один раз");
    }

    #[test]
    fn respawn_echo_ignored() {
        let mut l = logic();
        let s = sel(&["Germany"]);
        feed(&mut l, 1, "Hans (Leopard 2A7V) уничтожил Ivan (Т-90М)", &s);
        feed(&mut l, 2, "Ivan (Т-90М) уничтожил Hans (Leopard 2A7V)", &s);
        feed(&mut l, 3, "Hans (Leopard 2A7V) уничтожил Oleg (Т-90М)", &s);
        assert!(enemies(&l).is_empty());
    }

    #[test]
    fn ai_tag_and_timestamp_cleanup() {
        let mut l = logic();
        let s = sel(&["USSR"]);
        feed(&mut l, 1, "1:05 Bot ([ИИ] Pantsir-S1) сбил Joe (F-16C)", &s);
        assert_eq!(enemies(&l), vec!["Bot"]);
        assert_eq!(l.active[0].1.info.as_ref().unwrap().vtype, "zsu");
    }

    #[test]
    fn mq1_is_enemy_when_victim_is_ours() {
        let mut l = logic();
        let s = sel(&["Germany"]);
        feed(&mut l, 1, "Drone (MQ-1) уничтожил Ivan (Т-90М)", &s);
        assert_eq!(enemies(&l), vec!["Drone"]);
        assert_eq!(l.active[0].1.info.as_ref().unwrap().note, crate::t!("note.drone"));
        let mut l = logic();
        feed(&mut l, 1, "Drone (MQ-1) уничтожил Hans (Leopard 2A7V)", &s);
        assert!(enemies(&l).is_empty());
    }

    #[test]
    fn nuke_alert_once_and_air_disqualifies() {
        let mut l = logic();
        let s = sel(&["Germany"]);
        for i in 0..10 {
            feed(&mut l, i + 1, &format!("Hans (Leopard 2A7V) уничтожил V{i} (Т-90М)"), &s);
        }
        assert_eq!(l.take_sounds(), vec![SoundKind::Enemy]);
        assert!(l.overlay_lines(&cfg_all())[0].nuke);

        let mut l = logic();
        feed(&mut l, 1, "Joe (F-16C) сбил X (Ка-52)", &s);
        for i in 0..10 {
            feed(&mut l, i + 2, &format!("Joe (Т-90М) уничтожил V{i} (Т-90М)"), &s);
        }
        assert!(l.take_sounds().is_empty(), "игрок на авиации дисквалифицирован");
    }

    #[test]
    fn ally_nuke_sound() {
        let mut l = logic();
        let s = sel(&["Germany"]);
        for i in 0..9 {
            feed(&mut l, i + 1, &format!("Ivan (Т-90М) уничтожил V{i} (Leopard 2A7V)"), &s);
        }
        assert_eq!(l.take_sounds(), vec![SoundKind::Ally]);
    }

    #[test]
    fn history_skipped_and_battle_reset() {
        let mut l = Logic::new(VehicleDb::from_json(DB).unwrap());
        let s = sel(&["Germany"]);
        assert_eq!(l.hud_cursor(), None);
        l.on_mission(Some("running"), false);
        assert_eq!(l.hud_cursor(), None, "running без карты — это ангар после боя");
        l.on_mission(Some("running"), true);
        assert_eq!(l.hud_cursor(), Some(0));
        l.on_damage(&[HudMsg { id: 5, msg: "Hans (Leopard 2A7V) уничтожил Ivan (Т-90М)".into() }], &s);
        assert!(enemies(&l).is_empty(), "история не обрабатывается");
        assert_eq!(l.hud_cursor(), Some(5));
        feed(&mut l, 5, "Hans (Leopard 2A7V) уничтожил Ivan (Т-90М)", &s);
        assert!(enemies(&l).is_empty(), "старые id игнорируются");
        feed(&mut l, 6, "Hans (Leopard 2A7V) уничтожил Ivan (Т-90М)", &s);
        assert_eq!(enemies(&l), vec!["Hans"]);
        l.on_mission(None, false);
        l.on_mission(Some("running"), true);
        assert_eq!(enemies(&l), vec!["Hans"]);
        for _ in 0..LEAVE_POLLS {
            l.on_mission(Some("success"), false);
        }
        assert!(enemies(&l).is_empty());
        assert_eq!(l.hud_cursor(), None);
        l.on_mission(None, false);
        assert!(!l.api_available);
    }

    #[test]
    fn garbage_and_unknown() {
        let mut l = logic();
        let s = sel(&["Germany"]);
        feed(&mut l, 1, "Hans (Leopard 2A7V) уничтожил Ivan (Новый Танк)", &s);
        feed(&mut l, 2, "Hans (Leopard 2A7V) уничтожил x (weapons/agm_114)", &s);
        assert_eq!(l.unknown.iter().collect::<Vec<_>>(), vec!["Новый Танк"]);
    }

    fn cfg_all() -> Config {
        let mut c = Config::default();
        for n in crate::config::NATIONS {
            c.set_nation(n, true);
        }
        c
    }

    #[test]
    fn overlay_filters_and_format() {
        let mut l = logic();
        let s = sel(&["Germany", "USSR"]);
        feed(&mut l, 1, "Hans (Leopard 2A7V) уничтожил A (F-16C)", &s);
        feed(&mut l, 2, "Petr (Ка-52) уничтожил B (F-16C)", &s);
        let mut cfg = cfg_all();
        cfg.display.show_names = false;
        let lines = l.overlay_lines(&cfg);
        assert_eq!(lines[0].text, "🛡 Leopard 2A7V — ОБТ");
        assert_eq!(lines[1].text, "🚁 Ка-52");
        cfg.set_type("heli", false);
        cfg.display.show_notes = false;
        cfg.display.show_names = true;
        let lines = l.overlay_lines(&cfg);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "🛡 Leopard 2A7V (Hans)");
    }

    #[test]
    fn ambiguous_nations_list() {
        let db = r#"{"F-16C": {"nation": "unknown", "nations": ["USA", "Israel"], "type": "plane"},
                     "Т-90М": {"nation": "USSR", "type": "tank"}}"#;
        let mut l = Logic::new(VehicleDb::from_json(db).unwrap());
        l.on_mission(Some("running"), true);
        l.on_damage(&[], &sel(&[]));
        let s = sel(&["Israel"]);
        feed(&mut l, 1, "Joe (F-16C) сбил Ivan (Т-90М)", &s);
        assert_eq!(enemies(&l), vec!["Joe"]);
        let mut cfg = cfg_all();
        cfg.filters.nations = vec!["Israel".into()];
        assert_eq!(l.overlay_lines(&cfg).len(), 1);
        cfg.filters.nations = vec!["USSR".into()];
        assert!(l.overlay_lines(&cfg).is_empty());
    }

    #[test]
    fn unknown_vehicle_side_from_victim() {
        let mut l = logic();
        let s = sel(&["Germany"]);
        feed(&mut l, 1, "New (Leopard 3) уничтожил Ivan (Т-90М)", &s);
        feed(&mut l, 2, "New2 (Leopard 3) уничтожил Hans (Leopard 2A7V)", &s);
        assert_eq!(enemies(&l), vec!["New"]);
        assert!(!l.overlay_lines(&cfg_all())[0].text.starts_with("? "));
    }

    #[test]
    fn both_unknown_shown_as_uncertain_and_nuke_is_enemy() {
        let mut l = logic();
        let s = sel(&["Germany"]);
        for i in 0..9 {
            feed(&mut l, i + 1, &format!("Who (Mystery Tank) уничтожил V{i} (Other Tank)"), &s);
        }
        assert_eq!(l.side_of("Who"), None);
        assert!(l.overlay_lines(&cfg_all())[0].text.starts_with("☢"));
        assert_eq!(l.take_sounds(), vec![SoundKind::Enemy]);
        let mut l = logic();
        feed(&mut l, 1, "Who (Mystery Tank) уничтожил V (Other Tank)", &s);
        assert!(l.overlay_lines(&cfg_all())[0].text.starts_with("? "));
    }

    #[test]
    fn nothing_tracked_without_enemy_nations_but_nuke_alerts() {
        let mut l = logic();
        feed(&mut l, 1, "Hans (Leopard 2A7V) уничтожил Ivan (Т-90М)", &sel(&[]));
        feed(&mut l, 2, "Who (Mystery Tank) уничтожил V (Other Tank)", &sel(&[]));
        assert!(enemies(&l).is_empty());
        for i in 0..8 {
            feed(&mut l, i + 3, &format!("Hans (Leopard 2A7V) уничтожил V{i} (Т-90М)"), &sel(&[]));
        }
        assert_eq!(l.take_sounds(), vec![SoundKind::Enemy]);
    }

    #[test]
    fn other_client_languages_and_tricky_nicks() {
        let mut l = logic();
        let s = sel(&["Germany"]);
        feed(&mut l, 1, "Hans (Leopard 2A7V) zerstört Ivan (Т-90М)", &s);
        feed(&mut l, 2, "xX_сбил_Xx (Leopard 2A7V) уничтожил Ivan2 (Т-90М)", &s);
        assert_eq!(enemies(&l), vec!["Hans", "xX_сбил_Xx"]);
        feed(&mut l, 3, "Hans (Leopard 2A7V) ist abgestürzt.", &s);
        assert_eq!(enemies(&l), vec!["xX_сбил_Xx"]);
    }

    // ---- side detection in mixed battles ----

    const MIXED_DB: &str = r#"{
        "Strv 122": {"nation": "Sweden", "type": "tank", "operator": "sweden"},
        "Leopard 2A7V": {"nation": "Germany", "type": "tank", "operator": "germany_modern"},
        "M1A2": {"nation": "USA", "type": "tank", "operator": "usa_modern"},
        "Т-90М": {"nation": "USSR", "type": "tank", "operator": "russia"},
        "T-80B": {"nation": "USSR", "type": "tank", "operator": "ussr"},
        "Merkava Mk.4": {"nation": "Israel", "type": "tank", "operator": "israel"}
    }"#;

    // flags of src/scan/testdata/scoreboard_mixed_1440p.png
    const OWN: &[&str] = &["australia", "republic_china", "usa_modern", "japan", "ussr", "italy", "france", "china", "britain", "germany_modern", "argentina", "israel"];
    const ENEMY: &[&str] = &["usa_modern", "ussr", "france", "argentina", "britain", "japan", "italy", "republic_china", "china", "germany_modern", "sweden"];

    fn mixed() -> Logic {
        let mut l = Logic::new(VehicleDb::from_json(MIXED_DB).unwrap());
        l.on_mission(Some("running"), true);
        l.on_damage(&[], &sel(&[]));
        l
    }

    fn scanned(_l: &Logic) -> TeamContext {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        // trees as the scan maps them with the full database
        TeamContext {
            enemy_flags: ENEMY.iter().map(|f| canon_flag(f)).collect(),
            own_flags: OWN.iter().map(|f| canon_flag(f)).collect(),
            enemy_trees: s(&["USA", "USSR", "France", "Germany", "Great Britain", "Japan", "Italy", "China", "Sweden"]).into_iter().collect(),
            own_trees: s(&["Great Britain", "China", "USA", "Japan", "USSR", "Italy", "France", "Germany", "Israel"]).into_iter().collect(),
        }
    }

    fn enemy_cfg(ctx: &TeamContext) -> Config {
        let mut c = Config::default();
        for n in &ctx.enemy_trees {
            c.set_nation(n, true);
        }
        c
    }

    #[test]
    fn mixed_battle_from_screenshot() {
        let mut l = mixed();
        let ctx = scanned(&l);
        let cfg = enemy_cfg(&ctx);
        let all = sel(&ctx.enemy_trees.iter().map(String::as_str).collect::<Vec<_>>());
        l.set_teams(ctx);
        feed(&mut l, 1, "Swede (Strv 122) уничтожил German (Leopard 2A7V)", &all);
        feed(&mut l, 2, "German (Leopard 2A7V) уничтожил Yankee (M1A2)", &all);
        feed(&mut l, 3, "Comrade (T-80B) уничтожил Comrade2 (T-80B)", &all);
        feed(&mut l, 4, "Moshe (Merkava Mk.4) уничтожил Nobody (T-80B)", &all);
        assert_eq!(l.side_of("Swede"), Some(Side::Enemy), "sweden flag only on the enemy side");
        assert_eq!(l.side_of("German"), Some(Side::Ally), "killed by an enemy");
        assert_eq!(l.side_of("Yankee"), Some(Side::Enemy), "killed by an ally");
        assert_eq!(l.side_of("Comrade"), None);
        let lines: Vec<String> = l.overlay_lines(&cfg).into_iter().map(|x| x.text).collect();
        assert!(lines.iter().any(|t| t.contains("Strv 122") && !t.starts_with("? ")), "{lines:?}");
        assert!(lines.iter().any(|t| t.starts_with("? ") && t.contains("Comrade")), "{lines:?}");
        assert!(!lines.iter().any(|t| t.contains("Merkava")), "israel only on our side: {lines:?}");
    }

    #[test]
    fn nickname_anchors_sides() {
        for with_scan in [false, true] {
            let mut l = mixed();
            l.set_nickname("mrvokintos");
            if with_scan {
                let ctx = scanned(&l);
                l.set_teams(ctx);
            }
            feed(&mut l, 1, "[HARBS] mrvokintos (Leopard 2A7V) уничтожил X (Т-90М)", &sel(&[]));
            assert_eq!(l.side_of("[HARBS] mrvokintos"), Some(Side::Ally));
            assert_eq!(l.side_of("X"), Some(Side::Enemy), "scan: {with_scan}");
        }
        assert!(is_me("=TAG= Hans", "Hans") && is_me("Hans", "Hans"));
        assert!(!is_me("xHans", "Hans") && !is_me("hans", "Hans"));
    }

    #[test]
    fn ranks() {
        // a scan seed (Israel: ally-only flag) isn't overwritten by the graph
        let mut l = mixed();
        l.set_nickname("me");
        let ctx = scanned(&l);
        l.set_teams(ctx);
        feed(&mut l, 1, "me (Leopard 2A7V) уничтожил Moshe (Merkava Mk.4)", &sel(&[]));
        assert_eq!(l.side_of("Moshe"), Some(Side::Ally));
        assert_eq!(l.side_conflicts, 1);
        // the nickname beats a seed: driving an enemy-only vehicle is still us
        let mut l = mixed();
        l.set_nickname("me");
        let ctx = scanned(&l);
        l.set_teams(ctx);
        feed(&mut l, 1, "me (Strv 122) уничтожил Y (Т-90М)", &sel(&[]));
        assert_eq!(l.side_of("me"), Some(Side::Ally));
    }

    #[test]
    fn teamkill_is_a_conflict() {
        let mut l = mixed();
        let ctx = scanned(&l);
        l.set_teams(ctx);
        feed(&mut l, 1, "Swede (Strv 122) уничтожил Swede2 (Strv 122)", &sel(&[]));
        assert_eq!(l.side_of("Swede"), Some(Side::Enemy));
        assert_eq!(l.side_of("Swede2"), Some(Side::Enemy));
        assert_eq!(l.side_conflicts, 1);
    }

    #[test]
    fn scan_resolves_earlier_kills() {
        let mut l = mixed();
        feed(&mut l, 1, "A (Leopard 2A7V) уничтожил B (Strv 122)", &sel(&[]));
        feed(&mut l, 2, "C (T-80B) уничтожил A (Leopard 2A7V)", &sel(&[]));
        assert_eq!((l.side_of("A"), l.side_of("B"), l.side_of("C")), (None, None, None));
        let ctx = scanned(&l);
        l.set_teams(ctx);
        assert_eq!(l.side_of("B"), Some(Side::Enemy));
        assert_eq!(l.side_of("A"), Some(Side::Ally));
        assert_eq!(l.side_of("C"), Some(Side::Enemy));
    }

    #[test]
    fn new_battle_resets_sides() {
        let mut l = mixed();
        let ctx = scanned(&l);
        l.set_teams(ctx);
        feed(&mut l, 1, "Swede (Strv 122) уничтожил German (Leopard 2A7V)", &sel(&[]));
        assert!(l.side_of("Swede").is_some());
        for _ in 0..LEAVE_POLLS {
            l.on_mission(Some("success"), false);
        }
        assert!(l.side_of("Swede").is_none() && l.teams().is_none());
        assert!(l.edges.is_empty() && l.vehicles.is_empty() && l.side_conflicts == 0);
    }

    #[test]
    fn nuke_sound_follows_resolved_side() {
        let mut l = mixed();
        let ctx = scanned(&l);
        l.set_teams(ctx);
        feed(&mut l, 1, "G (Leopard 2A7V) уничтожил Swede (Strv 122)", &sel(&[]));
        for i in 0..8 {
            feed(&mut l, i + 2, &format!("G (Leopard 2A7V) уничтожил V{i} (T-80B)"), &sel(&[]));
        }
        assert_eq!(l.side_of("G"), Some(Side::Ally), "killed an enemy");
        assert_eq!(l.take_sounds(), vec![SoundKind::Ally]);
        for i in 0..9 {
            feed(&mut l, i + 20, &format!("U (T-80B) уничтожил W{i} (T-80B)"), &sel(&[]));
        }
        assert_eq!(l.side_of("U"), None);
        assert_eq!(l.take_sounds(), vec![SoundKind::Enemy]);
    }

    #[test]
    fn type_keys() {
        assert_eq!(type_key("aircraft"), "plane");
        assert_eq!(type_key("jet_fighter"), "plane");
        assert_eq!(type_key("SPAA"), "zsu");
        assert_eq!(type_key("whatever"), "special");
    }
}
