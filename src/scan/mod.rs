pub mod capture;
pub mod matcher;

use crate::paths;
use image::RgbImage;

pub struct ScanOutcome {
    pub nations: Vec<String>,
    pub own: Vec<String>,
    pub report: Vec<String>,
    pub black_frame: bool,
    /// the full search was used (no consistent row of flag places)
    pub fallback: bool,
    /// span of the row of flag places in the image: (first x, end x, flag width)
    pub row: Option<(u32, u32, u32)>,
}

/// Exclusive fullscreen on Windows (and some compositors) gives an all-black capture.
pub fn is_black(img: &image::RgbImage) -> bool {
    img.pixels().all(|p| p.0.iter().all(|&c| c <= 12))
}

pub fn scan_image(img: &RgbImage, confidence: f32) -> ScanOutcome {
    let templates = matcher::templates(&paths::file(paths::FLAGS_DIR));
    let scan = matcher::scan(img, &templates, confidence);
    let mut report: Vec<String> = scan.results.iter().map(|r| matcher::describe(r, confidence)).collect();
    if scan.fallback {
        report.push(crate::t!("report.fallback"));
    }
    let mut found: Vec<&matcher::MatchResult> = scan.results.iter().filter(|r| r.found).collect();
    found.sort_by_key(|r| r.pos.0);
    // the same flag can show up several times; keep the first occurrence
    let names = |v: &[&matcher::MatchResult]| {
        let mut out: Vec<String> = vec![];
        for r in v {
            if !out.contains(&r.name) {
                out.push(r.name.clone());
            }
        }
        out
    };
    let main = main_row(&scan.slots);
    let row = main.first().zip(main.last()).map(|(a, b)| (a.0, b.0 + b.2, main.iter().map(|s| s.2).max().unwrap_or(0)));
    let split = if scan.fallback {
        team_split(img, &found)
    } else {
        // places are known, unrecognised ones included: the team gap is the widest gap between them
        slot_split(&scan.slots).map(|x| found.iter().take_while(|r| r.pos.0 < x).count())
    };
    match split.filter(|&i| i > 0 && i < found.len()) {
        Some(i) => {
            report.push(crate::t!(
                "report.both_teams",
                own = format!("{:?}", names(&found[..i])),
                enemies = format!("{:?}", names(&found[i..]))
            ));
            ScanOutcome { nations: names(&found[i..]), own: names(&found[..i]), report, black_frame: false, fallback: scan.fallback, row }
        }
        None => ScanOutcome { nations: names(&found), own: vec![], report, black_frame: false, fallback: scan.fallback, row },
    }
}

/// x splitting the teams: middle of the widest gap between neighbouring slots, if it's at least
/// 0.4 flag widths (flags within a team are ~0.18 apart).
fn slot_split(slots: &[(u32, u32, u32, u32)]) -> Option<u32> {
    let v = main_row(slots);
    let fw = v.iter().map(|s| s.2).max()? as f32;
    // split in the middle of the gap: refined flag positions can be a pixel off the slot edge
    v.windows(2)
        .map(|p| {
            let end = p[0].0 + p[0].2;
            let gap = p[1].0.saturating_sub(end);
            (gap, end + gap / 2)
        })
        .filter(|&(gap, _)| gap as f32 >= 0.4 * fw)
        .max_by_key(|&(gap, _)| gap)
        .map(|(_, x)| x)
}

/// The longest run of places with gaps under 2 flag widths (the team gap is ~0.7): drops strays
/// from the rest of a widened capture, e.g. text on another monitor.
fn main_row(slots: &[(u32, u32, u32, u32)]) -> Vec<(u32, u32, u32, u32)> {
    let mut v = slots.to_vec();
    v.sort_by_key(|s| s.0);
    let Some(fw) = v.iter().map(|s| s.2).max() else { return v };
    let mut runs: Vec<Vec<(u32, u32, u32, u32)>> = vec![];
    for s in v {
        match runs.last_mut() {
            Some(run) if s.0.saturating_sub(run.last().map_or(0, |p| p.0 + p.2)) <= 2 * fw => run.push(s),
            _ => runs.push(vec![s]),
        }
    }
    runs.into_iter().max_by_key(|r| r.len()).unwrap_or_default()
}

fn flaglike(p: &image::Rgb<u8>) -> bool {
    let (_, s, v) = matcher::rgb_to_hsv(p[0], p[1], p[2]);
    (s > 110 && v > 70) || v > 225
}

// teams are split by an empty gap ~0.7 flag wide, flags inside a team are ~0.2 apart
pub fn team_split(img: &RgbImage, found: &[&matcher::MatchResult]) -> Option<usize> {
    if found.len() < 2 {
        return None;
    }
    let fw = found.iter().map(|r| r.size.0).max().unwrap_or(1) as f32;
    let y0 = found.iter().map(|r| r.pos.1).min().unwrap_or(0);
    let y1 = found.iter().map(|r| r.pos.1 + r.size.1).max().unwrap_or(0).min(img.height());
    let mut best: Option<(u32, usize)> = None;
    for i in 1..found.len() {
        let a = found[i - 1].pos.0 + found[i - 1].size.0;
        let b = (found[i].pos.0).min(img.width());
        if b <= a || ((b - a) as f32) < 0.4 * fw {
            continue;
        }
        let (mut run, mut longest) = (0u32, 0u32);
        for x in a..b {
            let colored = (y0..y1).filter(|&y| flaglike(img.get_pixel(x, y))).count() as f32;
            if colored / ((y1 - y0).max(1) as f32) < 0.08 {
                run += 1;
                longest = longest.max(run);
            } else {
                run = 0;
            }
        }
        if (longest as f32) >= 0.4 * fw && best.is_none_or(|(w, _)| longest > w) {
            best = Some((longest, i));
        }
    }
    best.map(|(_, i)| i)
}

/// Extra rows above and below the zone. A zone drawn tight around the flags has no background
/// band, the fast path can't find the flag places and the slow full search runs instead.
pub fn padded_region(r: [i32; 4]) -> ([i32; 4], i32) {
    let pad = ((r[3] as f32 * 0.35).round() as i32).max(6);
    // the linux capture starts at 0,0 and crop() clamps there; windows may have monitors above 0
    let top = if cfg!(windows) { r[1] - pad } else { (r[1] - pad).max(0) };
    ([r[0], top, r[2], r[3] + (r[1] - top) + pad], r[1] - top)
}

/// The zone as drawn if the fast path works on it, otherwise the padded capture if that helps.
pub fn pick_image(padded: RgbImage, inner_top: u32, inner_h: u32) -> RgbImage {
    let h = inner_h.min(padded.height().saturating_sub(inner_top));
    let inner = image::imageops::crop_imm(&padded, 0, inner_top, padded.width(), h).to_image();
    if matcher::has_slots(&inner) || !matcher::has_slots(&padded) {
        inner
    } else {
        log::debug!("[SCAN] zone is tight around the flags, using {} extra rows", padded.height() - h);
        padded
    }
}

/// The flag strip grows sideways with the number of nations, so a zone drawn in a small battle
/// cuts flags off in a big one. If the row of places reaches a side of the zone, widen the zone
/// on that side by a few flags. Small steps keep the capture near the flags, away from other monitors.
fn grow_zone(r: [i32; 4], row: (u32, u32, u32), img_w: u32) -> Option<[i32; 4]> {
    let (x0, x1, fw) = row;
    let near = fw + fw / 2;
    let left = x0 < near && (cfg!(windows) || r[0] > 0);
    let right = img_w.saturating_sub(x1) < near;
    if !left && !right {
        return None;
    }
    let pad = (fw * 3) as i32;
    let x = if left { if cfg!(windows) { r[0] - pad } else { (r[0] - pad).max(0) } } else { r[0] };
    Some([x, r[1], r[2] + (r[0] - x) + if right { pad } else { 0 }, r[3]])
}

pub fn scan_region(region: [i32; 4], confidence: f32) -> Result<ScanOutcome, String> {
    // linux takes a whole-desktop screenshot anyway: take it once, crop as often as needed
    #[cfg(not(windows))]
    let cap = capture::capture_full()?;
    let grab = |r: [i32; 4]| -> Result<RgbImage, String> {
        #[cfg(not(windows))]
        return capture::crop(&cap, r);
        #[cfg(windows)]
        return capture::capture_region(r);
    };
    let (img, out) = scan_zone(region, confidence, grab)?;
    for line in &out.report {
        log::debug!("[SCAN] {line}");
    }
    let p = paths::file(paths::DEBUG_SHOT);
    if let Err(e) = img.save(&p) {
        log::warn!("не удалось сохранить {}: {e}", p.display());
    }
    let _ = std::fs::write(paths::file(paths::REPORT_FILE), out.report.join("\n") + "\n");
    log::info!("[SCAN] {:?} (зона и отчёт: {})", out.nations, p.display());
    if out.fallback {
        log::info!("[SCAN] no consistent row of flag places, full search used");
    }
    Ok(out)
}

/// Scan a zone, widening it while the flags run into its sides. Returns the image scanned last.
fn scan_zone(region: [i32; 4], confidence: f32, grab: impl Fn([i32; 4]) -> Result<RgbImage, String>) -> Result<(RgbImage, ScanOutcome), String> {
    let mut r = region;
    let (mut img, mut out);
    let mut steps = 0;
    loop {
        let (padded, top) = padded_region(r);
        img = pick_image(grab(padded)?, top as u32, r[3].max(1) as u32);
        if is_black(&img) {
            out = ScanOutcome { nations: vec![], own: vec![], report: vec![crate::t!("scan.black")], black_frame: true, fallback: false, row: None };
            break;
        }
        out = scan_image(&img, confidence);
        let next = out.row.filter(|_| !out.fallback && steps < 8).and_then(|row| grow_zone(r, row, img.width()));
        // a zone at the edge of the screen can't grow: the capture stays the same width
        match next {
            Some(n) if steps == 0 || img.width() as i32 >= r[2] => {
                r = n;
                steps += 1;
            }
            _ => break,
        }
    }
    if steps > 0 {
        log::info!("[SCAN] flags reach the edge of the zone, widened it {steps}x to {r:?}");
    }
    Ok((img, out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::imageops::{overlay, resize, FilterType};
    use std::path::Path;

    // 48- and 50-star US flags look the same at 40x26; both are the USA tree
    fn canon(v: &[String]) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for x in v {
            let x = if x == "usa_modern" { "usa".to_string() } else { x.clone() };
            if !out.contains(&x) {
                out.push(x);
            }
        }
        out
    }

    fn strip(k: f32) -> RgbImage {
        let tpls = matcher::load_templates(Path::new("/nonexistent"));
        let get = |n: &str| tpls.iter().find(|t| t.name == n).unwrap().img.clone();
        let px = |v: f32| (v * k).round() as u32;
        let (fw, fh, gap, team_gap) = (px(40.0), px(26.0), px(7.0) as i64, px(28.0) as i64);
        let mut hay = RgbImage::from_pixel(px(520.0), px(40.0), image::Rgb([52, 60, 72]));
        let y = px(7.0) as i64;
        let mut x = px(25.0) as i64;
        for n in ["sweden", "usa", "france", "israel"] {
            overlay(&mut hay, &resize(&get(n), fw, fh, FilterType::Triangle), x, y);
            x += fw as i64 + gap;
        }
        let unknown = RgbImage::from_fn(fw, fh, |_, yy| {
            if yy > fh / 4 && yy < 3 * fh / 4 { image::Rgb([20, 40, 160]) } else { image::Rgb([200, 20, 30]) }
        });
        overlay(&mut hay, &unknown, x, y);
        x += fw as i64 + team_gap;
        for n in ["germany", "ussr", "britain"] {
            overlay(&mut hay, &resize(&get(n), fw, fh, FilterType::Triangle), x, y);
            x += fw as i64 + gap;
        }
        hay
    }

    #[test]
    fn splits_teams_at_any_resolution() {
        for k in [0.68f32, 0.8, 1.0, 1.25, 1.5, 1.6, 2.2] {
            let out = scan_image(&strip(k), 0.70);
            assert!(!out.fallback, "×{k}: full search used: {:#?}", out.report);
            assert_eq!(canon(&out.own), vec!["sweden", "usa", "france", "israel"], "×{k}: {:#?}", out.report);
            assert_eq!(canon(&out.nations), vec!["germany", "ussr", "britain"], "×{k}: {:#?}", out.report);
        }
    }

    /// Real scoreboard strip (1440p, mixed battle, operator flags such as Australia, Taiwan, FRG).
    #[test]
    fn real_scoreboard_operator_flags() {
        let img = image::load_from_memory(include_bytes!("testdata/scoreboard_mixed_1440p.png")).unwrap().to_rgb8();
        let out = scan_image(&img, 0.70);
        let want_own = ["australia", "republic_china", "usa", "japan", "ussr", "italy", "france", "china", "britain", "germany_modern", "argentina", "israel"];
        let want_enemy = ["usa", "ussr", "france", "argentina", "britain", "japan", "italy", "republic_china", "china", "germany_modern", "sweden"];
        assert_eq!(canon(&out.own), want_own, "{:#?}", out.report);
        assert_eq!(canon(&out.nations), want_enemy, "{:#?}", out.report);
    }

    /// Real capture scaled ×1.58 by the desktop: blurred USSR/China emblems score around 0.7.
    #[test]
    fn upscaled_capture_keeps_low_contrast_flags() {
        let img = image::load_from_memory(include_bytes!("testdata/scoreboard_upscaled_1440p.png")).unwrap().to_rgb8();
        let out = scan_image(&img, 0.70);
        let want_own = ["australia", "japan", "sweden", "italy", "britain", "ussr", "usa", "france", "germany_modern", "china", "republic_china"];
        let want_enemy = ["usa", "ussr", "italy", "britain", "china", "france", "japan", "sweden", "israel", "argentina"];
        assert_eq!(canon(&out.own), want_own, "{:#?}", out.report);
        assert_eq!(canon(&out.nations), want_enemy, "{:#?}", out.report);
    }

    #[test]
    fn stray_places_dont_move_the_team_split() {
        let row: Vec<(u32, u32, u32, u32)> = (0..6).map(|i| (100 + i * 47, 10, 40, 26)).chain((0..5).map(|i| (100 + 6 * 47 + 30 + i * 47, 10, 40, 26))).collect();
        let x = slot_split(&row).unwrap();
        let mut with_strays = row.clone();
        with_strays.extend([(3000, 12, 40, 26), (3060, 14, 44, 28)]);
        assert_eq!(slot_split(&with_strays), Some(x));
        assert!(row[5].0 + 40 <= x && x <= row[6].0);
    }

    #[test]
    fn zone_grows_only_towards_cut_flags() {
        let r = [1117, 408, 1828, 72];
        // first flag 15 px from the left edge, flags 63 px wide: a flag is probably cut off
        let g = grow_zone(r, (15, 1694, 63), 1828).unwrap();
        assert_eq!((g[0], g[0] + g[2], g[1], g[3]), (1117 - 189, 1117 + 1828, 408, 72));
        // room on both sides: nothing to do
        assert_eq!(grow_zone(r, (200, 1600, 63), 1828), None);
        // right side
        let g = grow_zone(r, (200, 1800, 63), 1828).unwrap();
        assert_eq!((g[0], g[2]), (1117, 1828 + 189));
        // the real cut-off capture: Australia is missing on the left
        let img = image::load_from_memory(include_bytes!("testdata/scoreboard_upscaled_1440p.png")).unwrap().to_rgb8();
        let out = scan_image(&img, 0.70);
        assert!(grow_zone(r, out.row.unwrap(), img.width()).is_some(), "{:?}", out.row);
    }

    /// Event battle (WWI): one team flag per side, surrounded by the score text.
    #[test]
    fn event_battle_team_flags() {
        let img = image::load_from_memory(include_bytes!("testdata/scoreboard_event_entente.png")).unwrap().to_rgb8();
        let out = scan_image(&img, 0.70);
        assert!(!out.fallback, "{:#?}", out.report);
        assert_eq!(out.own, ["germany_empire"], "{:#?}", out.report);
        assert_eq!(out.nations, ["triple_entente"], "{:#?}", out.report);
    }

    /// Same battle from the other side: the German Empire's black stripe is drawn almost background grey.
    #[test]
    fn event_battle_dark_stripe_flag() {
        let img = image::load_from_memory(include_bytes!("testdata/scoreboard_event_entente_dark.png")).unwrap().to_rgb8();
        let out = scan_image(&img, 0.70);
        assert!(!out.fallback, "{:#?}", out.report);
        let mut all = [out.own.clone(), out.nations.clone()].concat();
        all.sort();
        assert_eq!(all, ["germany_empire", "triple_entente"], "{:#?}", out.report);
    }

    #[test]
    fn team_context_from_real_scoreboard() {
        let img = image::load_from_memory(include_bytes!("testdata/scoreboard_mixed_1440p.png")).unwrap().to_rgb8();
        let out = scan_image(&img, 0.70);
        let db = crate::vehicles::VehicleDb::from_json(crate::paths::EMBEDDED_VEHICLES).unwrap();
        let ctx = crate::logic::Logic::new(db).team_context(&out.nations, &out.own);
        let set = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<std::collections::BTreeSet<_>>();
        let both: std::collections::BTreeSet<String> = ctx.enemy_trees.intersection(&ctx.own_trees).cloned().collect();
        let enemy_only: std::collections::BTreeSet<String> = ctx.enemy_trees.difference(&ctx.own_trees).cloned().collect();
        assert_eq!(both, set(&["USA", "USSR", "France", "Great Britain", "Japan", "Italy", "China", "Germany"]));
        assert_eq!(enemy_only, set(&["Sweden"]));
    }

    #[test]
    fn real_scoreboard_slots() {
        let img = image::load_from_memory(include_bytes!("testdata/scoreboard_mixed_1440p.png")).unwrap().to_rgb8();
        let templates = matcher::templates(Path::new("/nonexistent"));
        let out = matcher::scan(&img, &templates, 0.70);
        assert!(!out.fallback);
        assert_eq!(out.slots.len(), 23);
        let mut slots = out.slots.clone();
        slots.sort_by_key(|s| s.0);
        let x = slot_split(&slots).unwrap();
        assert!(slots[11].0 + slots[11].2 <= x && x <= slots[12].0, "team gap between the 12th and 13th place");
    }

    #[test]
    fn no_places_falls_back_to_full_search() {
        let tpls = matcher::load_templates(Path::new("/nonexistent"));
        let mut hay = RgbImage::from_fn(300, 40, |x, _| image::Rgb([(x * 255 / 300) as u8, 120, (255 - x * 255 / 300) as u8]));
        let t = &tpls.iter().find(|t| t.name == "japan").unwrap().img;
        overlay(&mut hay, t, 130, 7);
        let out = scan_image(&hay, 0.70);
        assert!(out.fallback);
        assert!(out.report.iter().any(|l| *l == crate::t!("report.fallback")), "{:#?}", out.report);
    }

    /// cargo test --release -- --ignored
    #[test]
    #[ignore]
    fn scan_speed_release() {
        let img = image::load_from_memory(include_bytes!("testdata/scoreboard_mixed_1440p.png")).unwrap().to_rgb8();
        let templates = matcher::templates(Path::new("/nonexistent"));
        let t = std::time::Instant::now();
        let out = matcher::scan(&img, &templates, 0.70);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        println!("scan: {ms:.1} ms, {} places", out.slots.len());
        assert!(ms < 100.0, "{ms:.1} ms");
    }

    #[test]
    fn tight_zone_uses_padding_and_fast_path() {
        let full = image::load_from_memory(include_bytes!("testdata/scoreboard_mixed_1440p.png")).unwrap().to_rgb8();
        // flags occupy rows 14..40; a zone drawn 2 px around them has no background band
        let tight = image::imageops::crop_imm(&full, 0, 12, full.width(), 31).to_image();
        assert!(!matcher::has_slots(&tight), "tight crop must need padding for this test to mean anything");
        let ([_, top, _, h], inner_top) = padded_region([0, 12, full.width() as i32, 31]);
        let padded = image::imageops::crop_imm(&full, 0, top as u32, full.width(), h as u32).to_image();
        let img = pick_image(padded, inner_top as u32, 31);
        let out = scan_image(&img, 0.70);
        assert!(!out.fallback, "{:#?}", out.report);
        assert_eq!(out.own.len() + out.nations.len(), 23, "{:#?}", out.report);
        // a normal zone is used as is
        let same = pick_image(full.clone(), 0, full.height());
        assert_eq!(same.dimensions(), full.dimensions());
    }

    #[test]
    fn black_frame_detected() {
        assert!(is_black(&RgbImage::from_pixel(100, 30, image::Rgb([3, 3, 5]))));
        assert!(!is_black(&strip(1.0)));
    }

    #[test]
    fn single_team_not_split() {
        let tpls = matcher::load_templates(Path::new("/nonexistent"));
        let mut hay = RgbImage::from_pixel(300, 40, image::Rgb([52, 60, 72]));
        for (i, n) in ["germany", "ussr", "japan"].iter().enumerate() {
            let t = &tpls.iter().find(|t| t.name == *n).unwrap().img;
            overlay(&mut hay, t, 10 + i as i64 * 50, 7);
        }
        let out = scan_image(&hay, 0.70);
        assert!(out.own.is_empty());
        assert_eq!(out.nations.len(), 3, "{:#?}", out.report);
    }
}

