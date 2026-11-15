// generated from the game's lang/ui.csv (NET_UNIT_KILLED_GM/FM, NET_PLAYER_HAS_CRASHED,
// NET_PLAYER_GM_HAS_DESTROYED, NET_PLAYER_SHOT_BY_AIR_DEFENCE), all 21 client languages.
// The fragments have no placeholders: the game always builds "killer (veh) <verb> victim (veh)",
// which is why some of them end in "a", "pe" or ":". The \t marks in Japanese are line-break
// hints, so both "with space" and "without space" variants are listed.

pub const KILL_VERBS: &[&str] = &[
    "megsemmisítette:",
    "ha derribado a",
    "l-a doborât pe",
    "ha destruido a",
    "l-a distrus pe",
    "ha abbattuto",
    "abgeschossen",
    "ha distrutto",
    "によって 撃破されました",
    "によって撃破されました",
    "zestrzelił",
    "imha etti:",
    "đã phá huỷ",
    "shot down",
    "sestřelil",
    "destroyed",
    "уничтожил",
    "zniszczył",
    "düşürdü:",
    "lelőtte:",
    "zerstört",
    "destruiu",
    "撃墜されました",
    "détruit",
    "uništio",
    "знішчыў",
    "abattu",
    "abateu",
    "oborio",
    "bắn hạ",
    "zničil",
    "знищив",
    "сбил",
    "збив",
    "격추 ↓",
    "збіў",
    "격파 ↓",
    "击落了",
    "擊落了",
    "击毁了",
    "擊毀了",
];

pub const DEATH_VERBS: &[&str] = &[
    "zestrzelony przez obronę przeciwlotniczą.",
    "Abgeschossen durch Luftverteidigung.",
    "abattu par la défense antiaérienne.",
    "derribado por la defensa antiaérea.",
    "hava savunma tarafından düşürüldü",
    "Sestřelen protivzdušnou obranou.",
    "abbatatuto dalla contraerea.",
    "đã bị bắn hạ bởi phòng không",
    "abatido pela defesa aérea.",
    "shot down by air defense.",
    "járműve totálkáros lett.",
    "légvédelem által lelőve.",
    "sofreu um acidente.",
    "összetörte a gépét.",
    "防空 システムにより 撃墜されました。",
    "se ha estrellado.",
    "è stato distrutto",
    "ha sido destruido",
    "防空システムにより撃墜されました。",
    "si è schiantato.",
    "has been wrecked",
    "выведен из строя",
    "виведений з ладу",
    "대공포에 의해 격추당했습니다.",
    "ist abgestürzt.",
    "a fost distrusă",
    "самоуничтожился",
    "wurde zerstört",
    "został rozbity",
    "s'est écrasé.",
    "yere çakıldı.",
    "s-a prăbușit.",
    "a été détruit",
    "foi destruido",
    "has crashed.",
    "이(가) 파괴되었습니다",
    "이(가) 추락했습니다",
    "imha edildi",
    "быў разбіты",
    "збитий ППО.",
    "rozbił się",
    "se srušio.",
    "byl zničen",
    "je uništen",
    "đã bị hỏng",
    "збіты СПА.",
    "havaroval",
    "розбився.",
    "сбит ПВО.",
    "被地面防空火力击落",
    "被地面防空火力擊落",
    "разбился",
    "は 墜落しました",
    "разбіўся",
    "は墜落しました",
    "bị rơi.",
    "離脱しました",
    "已坠毁。",
    "已被摧毁",
    "已被摧毀",
    "已墜毀",
];

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Kill { killer: String, victim: String },
    Death { who: String },
}

pub fn normalize(text: &str) -> String {
    let t = text.replace('\u{200b}', "").replace("\\t", " ").replace(['\t', '\n', '\r'], " ");
    // only plain spaces: vehicle names can contain a no-break space
    let mut out = String::with_capacity(t.len());
    for c in t.trim_matches(' ').chars() {
        if c == ' ' && out.ends_with(' ') {
            continue;
        }
        out.push(c);
    }
    out
}

/// CJK text doesn't put spaces around the verb.
fn needs_space(verb: &str) -> bool {
    verb.chars().last().is_some_and(|c| c.is_alphabetic() && (c as u32) < 0x2E80)
}

/// Parses "A (veh) <verb> B (veh)" and "A (veh) <verb>". The verb only counts right after
/// a closing paren, so nicknames containing a verb don't break the split.
pub fn parse(text: &str) -> Option<Event> {
    let t = normalize(text);
    let trim_punct = |s: &str| s.trim_end_matches(['.', '。', ' ']).to_string();
    let bare = trim_punct(&t);
    for v in DEATH_VERBS {
        if let Some(rest) = bare.strip_suffix(trim_punct(v).as_str()) {
            let who = rest.trim_end();
            if who.ends_with(')') {
                return Some(Event::Death { who: who.to_string() });
            }
        }
    }
    let mut best: Option<(usize, String, String)> = None;
    for v in KILL_VERBS {
        for (idx, _) in t.match_indices(v) {
            if best.as_ref().is_some_and(|b| b.0 <= idx) {
                break;
            }
            let before = t[..idx].trim_end();
            let after = &t[idx + v.len()..];
            let boundary = after.is_empty() || after.starts_with(' ') || !needs_space(v);
            if before.ends_with(')') && boundary && !after.trim().is_empty() {
                best = Some((idx, before.to_string(), after.trim().to_string()));
                break;
            }
        }
    }
    best.map(|(_, killer, victim)| Event::Kill { killer, victim })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kill(k: &str, v: &str) -> Option<Event> {
        Some(Event::Kill { killer: k.into(), victim: v.into() })
    }

    #[test]
    fn languages() {
        assert_eq!(parse("Hans (Leopard 2A7V) уничтожил Ivan (Т-90М)"), kill("Hans (Leopard 2A7V)", "Ivan (Т-90М)"));
        assert_eq!(parse("Hans (Leopard 2A7V) zerstört Ivan (T-90M)"), kill("Hans (Leopard 2A7V)", "Ivan (T-90M)"));
        assert_eq!(parse("Jan (F-16C) zestrzelił Bob (MiG-29)"), kill("Jan (F-16C)", "Bob (MiG-29)"));
        assert_eq!(parse("Li (ZTZ99A) 击\u{200b}毁\u{200b}了 Bob (M1A2)"), kill("Li (ZTZ99A)", "Bob (M1A2)"));
        assert_eq!(parse("Joe (F-16C) has crashed."), Some(Event::Death { who: "Joe (F-16C)".into() }));
        assert_eq!(parse("Joe (F-16C) ist abgestürzt."), Some(Event::Death { who: "Joe (F-16C)".into() }));
        assert_eq!(parse("Joe (Ka-52) сбит ПВО."), Some(Event::Death { who: "Joe (Ka-52)".into() }));
        assert_eq!(parse("A (Mosquito\u{a0}FB) сбил B (Bf 109)"), kill("A (Mosquito\u{a0}FB)", "B (Bf 109)"));
    }

    #[test]
    fn every_kill_fragment() {
        for v in KILL_VERBS {
            let msg = format!("Hans (Leopard 2A7V) {v} Ivan (T-90M)");
            assert_eq!(parse(&msg), kill("Hans (Leopard 2A7V)", "Ivan (T-90M)"), "{msg}");
            if !needs_space(v) {
                let msg = format!("Hans (Leopard 2A7V){v}Ivan (T-90M)");
                assert_eq!(parse(&msg), kill("Hans (Leopard 2A7V)", "Ivan (T-90M)"), "{msg}");
            }
        }
    }

    #[test]
    fn every_death_fragment() {
        for v in DEATH_VERBS {
            for msg in [format!("Joe (F-16C) {v}"), format!("Joe (F-16C){v}")] {
                assert_eq!(parse(&msg), Some(Event::Death { who: "Joe (F-16C)".into() }), "{msg}");
            }
        }
    }

    #[test]
    fn japanese_and_korean_layouts() {
        // \t rendered as a tab, as nothing, and the victim on a new line
        assert_eq!(parse("A (Type 10)によって\t撃破されました B (T-90M)"), kill("A (Type 10)", "B (T-90M)"));
        assert_eq!(parse("A (Type 10)によって撃破されましたB (T-90M)"), kill("A (Type 10)", "B (T-90M)"));
        assert_eq!(parse("A (K2) 격파 ↓\nB (T-90M)"), kill("A (K2)", "B (T-90M)"));
        assert_eq!(parse("A (F-2A)は\t墜落しました"), Some(Event::Death { who: "A (F-2A)".into() }));
    }

    #[test]
    fn verb_inside_nickname() {
        assert_eq!(parse("xX_сбил_Xx (Т-90М) уничтожил Bob (M1A2)"), kill("xX_сбил_Xx (Т-90М)", "Bob (M1A2)"));
        assert_eq!(parse("Bob (M1A2) destroyed Destroyed_U (T-90M)"), kill("Bob (M1A2)", "Destroyed_U (T-90M)"));
        assert_eq!(parse("Bob (M1A2) destroyed has crashed (T-90M)"), kill("Bob (M1A2)", "has crashed (T-90M)"));
    }

    #[test]
    fn death_phrase_is_not_a_kill() {
        // "abattu" is also a kill verb in French
        assert_eq!(
            parse("Jean (Rafale) abattu par la défense antiaérienne."),
            Some(Event::Death { who: "Jean (Rafale)".into() })
        );
    }

    #[test]
    fn unrelated_messages() {
        assert_eq!(parse("[HARBS] mrvokintos (M1A1 SA) получил \"Помощь с починкой: x2\""), None);
        assert_eq!(parse("Bob has crashed."), None);
    }
}
