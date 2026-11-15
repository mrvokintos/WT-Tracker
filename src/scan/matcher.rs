use image::RgbImage;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct Template {
    pub name: String,
    pub img: RgbImage,
    gray: Gray,
    sig: Sig,
}

impl Template {
    pub fn new(name: String, img: RgbImage) -> Self {
        Self { gray: Gray::from(&img), sig: signature(&img), name, img }
    }
}

/// Mean colour on a 4x3 grid: a cheap fingerprint to shortlist templates for a flag slot.
type Sig = [f32; 36];

fn signature(img: &RgbImage) -> Sig {
    let (w, h) = (img.width().max(1), img.height().max(1));
    let mut sum = [0f32; 36];
    let mut n = [0f32; 12];
    for (x, y, p) in img.enumerate_pixels() {
        let cell = ((y * 3 / h).min(2) * 4 + (x * 4 / w).min(3)) as usize;
        n[cell] += 1.0;
        for c in 0..3 {
            sum[cell * 3 + c] += p[c] as f32;
        }
    }
    for cell in 0..12 {
        for c in 0..3 {
            sum[cell * 3 + c] /= n[cell].max(1.0);
        }
    }
    sum
}

fn sig_dist(a: &Sig, b: &Sig) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// Templates are decoded once per folder.
pub fn templates(dir: &Path) -> Arc<Vec<Template>> {
    static CACHE: Mutex<Vec<(PathBuf, Arc<Vec<Template>>)>> = Mutex::new(Vec::new());
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, t)) = cache.iter().find(|(p, _)| p == dir) {
        return t.clone();
    }
    let t = Arc::new(load_templates(dir));
    cache.push((dir.to_path_buf(), t.clone()));
    t
}

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub name: String,
    pub score: Option<f32>,
    pub scale: f32,
    pub pos: (u32, u32),
    pub size: (u32, u32),
    pub color_ok: bool,
    pub found: bool,
    pub note: &'static str,
    pub color_dist: f32,
    /// next best templates for this place (name, score), for the report
    pub alts: Vec<(String, f32)>,
}

/// Result of a scan: recognised places and how they were found.
#[derive(Debug)]
pub struct ScanOutput {
    pub results: Vec<MatchResult>,
    /// flag places (x, y, w, h); empty when the full search was used
    pub slots: Vec<(u32, u32, u32, u32)>,
    pub fallback: bool,
}

// real flags ~30-80 even from a blurry jpeg, grass and similar junk 125+
pub const MAX_COLOR_DIST: f32 = 100.0;

// ccoeff normalizes every channel, so a striped green field can pass for italy
pub fn color_distance(crop: &RgbImage, tpl: &RgbImage) -> f32 {
    let c = image::imageops::resize(crop, tpl.width(), tpl.height(), image::imageops::FilterType::Triangle);
    let mut d: Vec<u8> = c
        .pixels()
        .zip(tpl.pixels())
        .map(|(a, b)| (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap_or(0))
        .collect();
    if d.is_empty() {
        return 255.0;
    }
    let mid = d.len() / 2;
    *d.select_nth_unstable(mid).1 as f32
}

pub fn load_templates(dir: &Path) -> Vec<Template> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            if !matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
                continue;
            }
            let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            match image::open(&p) {
                Ok(img) => out.push(Template::new(name, img.to_rgb8())),
                Err(e) => log::warn!("Не удалось загрузить {}: {e}", p.display()),
            }
        }
    }
    if out.is_empty() {
        for (name, bytes) in crate::paths::EMBEDDED_FLAGS {
            if let Ok(img) = image::load_from_memory(bytes) {
                out.push(Template::new(name.to_string(), img.to_rgb8()));
            }
        }
    }
    out
}

pub fn match_template(hay: &RgbImage, tpl: &RgbImage) -> Option<(f32, u32, u32)> {
    let (hw, hh) = (hay.width() as usize, hay.height() as usize);
    let (tw, th) = (tpl.width() as usize, tpl.height() as usize);
    if tw == 0 || th == 0 || tw > hw || th > hh {
        return None;
    }
    let n = (tw * th) as f64;

    let mut mean = [0f64; 3];
    for p in tpl.pixels() {
        for c in 0..3 {
            mean[c] += p[c] as f64;
        }
    }
    for m in &mut mean {
        *m /= n;
    }
    let mut tprime = vec![0f32; tw * th * 3];
    let mut t_norm2 = 0f64;
    for (i, p) in tpl.pixels().enumerate() {
        for c in 0..3 {
            let v = p[c] as f64 - mean[c];
            tprime[i * 3 + c] = v as f32;
            t_norm2 += v * v;
        }
    }

    let iw = hw + 1;
    let mut sum = vec![[0f64; 3]; iw * (hh + 1)];
    let mut sq = vec![[0f64; 3]; iw * (hh + 1)];
    let raw = hay.as_raw();
    for y in 0..hh {
        let mut row = [0f64; 3];
        let mut row2 = [0f64; 3];
        for x in 0..hw {
            for c in 0..3 {
                let v = raw[(y * hw + x) * 3 + c] as f64;
                row[c] += v;
                row2[c] += v * v;
                sum[(y + 1) * iw + x + 1][c] = sum[y * iw + x + 1][c] + row[c];
                sq[(y + 1) * iw + x + 1][c] = sq[y * iw + x + 1][c] + row2[c];
            }
        }
    }
    let rect = |t: &Vec<[f64; 3]>, x: usize, y: usize, c: usize| {
        t[(y + th) * iw + x + tw][c] - t[y * iw + x + tw][c] - t[(y + th) * iw + x][c] + t[y * iw + x][c]
    };

    let mut best = (f32::MIN, 0u32, 0u32);
    for y in 0..=(hh - th) {
        for x in 0..=(hw - tw) {
            let mut i_var = 0f64;
            for c in 0..3 {
                let s = rect(&sum, x, y, c);
                i_var += rect(&sq, x, y, c) - s * s / n;
            }
            let denom = (t_norm2 * i_var.max(0.0)).sqrt();
            let score = if denom < 1e-6 {
                0.0
            } else {
                let mut num = 0f32;
                for ty in 0..th {
                    let hrow = &raw[((y + ty) * hw + x) * 3..((y + ty) * hw + x + tw) * 3];
                    let trow = &tprime[ty * tw * 3..(ty + 1) * tw * 3];
                    num += hrow.iter().zip(trow).map(|(&h, &t)| h as f32 * t).sum::<f32>();
                }
                (num as f64 / denom) as f32
            };
            if score > best.0 {
                best = (score, x as u32, y as u32);
            }
        }
    }
    Some(best)
}

pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let (rf, gf, bf) = (r as f32, g as f32, b as f32);
    let v = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let diff = v - min;
    let s = if v > 0.0 { diff * 255.0 / v } else { 0.0 };
    let mut h = if diff == 0.0 {
        0.0
    } else if v == rf {
        60.0 * (gf - bf) / diff
    } else if v == gf {
        120.0 + 60.0 * (bf - rf) / diff
    } else {
        240.0 + 60.0 * (rf - gf) / diff
    };
    if h < 0.0 {
        h += 360.0;
    }
    ((h / 2.0).round().min(180.0) as u8, s.round() as u8, v as u8)
}

type HsvRange = ((u8, u8, u8), (u8, u8, u8));
const BLUE: HsvRange = ((90, 50, 50), (130, 255, 255));
const GREEN: HsvRange = ((35, 50, 50), (85, 255, 255));
const RED1: HsvRange = ((0, 50, 50), (10, 255, 255));
const RED2: HsvRange = ((170, 50, 50), (180, 255, 255));

fn in_range(p: (u8, u8, u8), r: &HsvRange) -> bool {
    let ((h0, s0, v0), (h1, s1, v1)) = *r;
    (h0..=h1).contains(&p.0) && (s0..=s1).contains(&p.1) && (v0..=v1).contains(&p.2)
}

fn share(hsv: &[(u8, u8, u8)], ranges: &[&HsvRange]) -> f32 {
    if hsv.is_empty() {
        return 0.0;
    }
    hsv.iter().filter(|p| ranges.iter().any(|r| in_range(**p, r))).count() as f32 / hsv.len() as f32
}

pub fn color_check(crop: &RgbImage, name: &str) -> bool {
    let hsv: Vec<_> = crop.pixels().map(|p| rgb_to_hsv(p[0], p[1], p[2])).collect();
    match name.to_lowercase().as_str() {
        "italy" => share(&hsv, &[&GREEN]) > 0.15 && share(&hsv, &[&BLUE]) < 0.05,
        "france" => share(&hsv, &[&BLUE]) > 0.15 && share(&hsv, &[&GREEN]) < 0.05,
        "usa" | "usa_modern" => share(&hsv, &[&BLUE]) > 0.08 && share(&hsv, &[&RED1, &RED2]) > 0.25,
        _ => true,
    }
}

struct Gray {
    w: usize,
    h: usize,
    px: Vec<f32>,
}

impl Gray {
    fn from(img: &RgbImage) -> Self {
        let px = img
            .pixels()
            .map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32)
            .collect();
        Self { w: img.width() as usize, h: img.height() as usize, px }
    }
}

fn gray_best(hay: &Gray, tpl: &Gray, win: Option<(usize, usize, usize, usize)>) -> Option<(f32, usize, usize)> {
    let (tw, th) = (tpl.w, tpl.h);
    if tw == 0 || th == 0 || tw > hay.w || th > hay.h {
        return None;
    }
    let n = (tw * th) as f64;
    let mean = tpl.px.iter().map(|&v| v as f64).sum::<f64>() / n;
    let tp: Vec<f32> = tpl.px.iter().map(|&v| (v as f64 - mean) as f32).collect();
    let t2: f64 = tp.iter().map(|&v| (v as f64).powi(2)).sum();
    if t2 < 1e-6 {
        return None;
    }
    let (mx, my) = (hay.w - tw, hay.h - th);
    let (x0, y0, x1, y1) = match win {
        Some((a, b, c, d)) => (a.min(mx), b.min(my), c.min(mx), d.min(my)),
        None => (0, 0, mx, my),
    };
    let iw = hay.w + 1;
    let mut s1 = vec![0f64; iw * (hay.h + 1)];
    let mut s2 = vec![0f64; iw * (hay.h + 1)];
    for y in 0..hay.h {
        let (mut r1, mut r2) = (0f64, 0f64);
        for x in 0..hay.w {
            let v = hay.px[y * hay.w + x] as f64;
            r1 += v;
            r2 += v * v;
            s1[(y + 1) * iw + x + 1] = s1[y * iw + x + 1] + r1;
            s2[(y + 1) * iw + x + 1] = s2[y * iw + x + 1] + r2;
        }
    }
    let rect = |t: &[f64], x: usize, y: usize| {
        t[(y + th) * iw + x + tw] - t[y * iw + x + tw] - t[(y + th) * iw + x] + t[y * iw + x]
    };
    let mut best: Option<(f32, usize, usize)> = None;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let s = rect(&s1, x, y);
            let var = rect(&s2, x, y) - s * s / n;
            if var < 1e-3 {
                continue;
            }
            let mut num = 0f32;
            for ty in 0..th {
                let h = &hay.px[(y + ty) * hay.w + x..(y + ty) * hay.w + x + tw];
                let t = &tp[ty * tw..(ty + 1) * tw];
                num += h.iter().zip(t).map(|(a, b)| a * b).sum::<f32>();
            }
            let score = (num as f64 / (t2 * var).sqrt()) as f32;
            if best.is_none_or(|b| score > b.0) {
                best = Some((score, x, y));
            }
        }
    }
    best
}

fn rgb_at(hay: &RgbImage, tpl: &RgbImage, x: u32, y: u32) -> f32 {
    let crop = image::imageops::crop_imm(hay, x, y, tpl.width(), tpl.height()).to_image();
    match_template(&crop, tpl).map_or(0.0, |r| r.0)
}

fn resize(img: &RgbImage, w: u32, h: u32) -> RgbImage {
    image::imageops::resize(img, w.max(1), h.max(1), image::imageops::FilterType::Triangle)
}

#[derive(Debug, Clone, Copy)]
struct Cand {
    score: f32,
    scale: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn find_candidates(hay: &RgbImage, templates: &[Template]) -> Vec<(usize, Cand)> {
    let (hw, hh) = (hay.width() as f32, hay.height() as f32);
    let min_th = templates.iter().map(|t| t.img.height()).min().unwrap_or(1) as f32;
    let s_max = (hh / min_th).min(4.0);
    let s_min = (0.25 * hh / min_th).max(0.3).min(s_max);
    let mut scales = vec![];
    let mut s = s_min;
    while s <= s_max * 1.0001 {
        scales.push(s);
        s *= 1.05;
    }
    if scales.is_empty() {
        return vec![];
    }
    const C: f32 = 0.5;
    let tpl_c: Vec<Gray> = templates
        .iter()
        .map(|t| {
            let w = (t.img.width() as f32 * C).round() as u32;
            let h = (t.img.height() as f32 * C).round() as u32;
            Gray::from(&resize(&t.img, w, h))
        })
        .collect();

    let threads = max_threads().min(scales.len());
    let chunks: Vec<Vec<f32>> = (0..threads).map(|i| scales.iter().copied().skip(i).step_by(threads).collect()).collect();
    type Coarse = Vec<Option<(f32, f32, usize, usize)>>;
    let coarse: Vec<Coarse> = std::thread::scope(|sc| {
        let handles: Vec<_> = chunks
            .iter()
            .map(|ch| {
                let tpl_c = &tpl_c;
                sc.spawn(move || {
                    let mut best = vec![None::<(f32, f32, usize, usize)>; tpl_c.len()];
                    for &s in ch {
                        let g = Gray::from(&resize(hay, (hw * C / s).round() as u32, (hh * C / s).round() as u32));
                        for (i, t) in tpl_c.iter().enumerate() {
                            if let Some((sc, x, y)) = gray_best(&g, t, None) {
                                if best[i].is_none_or(|b| sc > b.0) {
                                    best[i] = Some((sc, s, x, y));
                                }
                            }
                        }
                    }
                    best
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let refined: Vec<Option<Cand>> = templates
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let (_, s0, cx, cy) = coarse.iter().filter_map(|v| v[i]).max_by(|a, b| a.0.total_cmp(&b.0))?;
            let tg = Gray::from(&t.img);
            let mut best: Option<Cand> = None;
            for s in [s0 / 1.05, s0, s0 * 1.05] {
                let (rw, rh) = ((hw / s).round() as u32, (hh / s).round() as u32);
                if rw < t.img.width() || rh < t.img.height() {
                    continue;
                }
                let r = resize(hay, rw, rh);
                let k = s0 / (C * s);
                let (gx, gy) = ((cx as f32 * k) as usize, (cy as f32 * k) as usize);
                let win = (gx.saturating_sub(4), gy.saturating_sub(4), gx + 4, gy + 4);
                let Some((_, x, y)) = gray_best(&Gray::from(&r), &tg, Some(win)) else { continue };
                let score = rgb_at(&r, &t.img, x as u32, y as u32);
                if best.is_none_or(|b| score > b.score) {
                    best = Some(cand(score, s, x as u32, y as u32, &t.img));
                }
            }
            best
        })
        .collect();

    // all flags in the strip are the same size -> redo everything at the median scale.
    // flat flags (ussr, japan) often get the wrong scale in the luma pass
    // consensus from the hits close to the best one: with many templates, pieces of a flag match
    // small templates at small scales (~0.8), while the real flags match almost perfectly
    let best = refined.iter().flatten().map(|c| c.score).fold(0f32, f32::max);
    let cut = (best - 0.08).max(0.6);
    let mut strong: Vec<f32> = refined.iter().flatten().filter(|c| c.score >= cut).map(|c| c.scale).collect();
    if strong.is_empty() {
        return refined.into_iter().enumerate().filter_map(|(i, c)| c.map(|c| (i, c))).collect();
    }
    strong.sort_by(f32::total_cmp);
    let med = strong[strong.len() / 2];
    // every template again around the consensus scale; keep all peaks, the same flag can be in both teams.
    // three neighbouring scales: near-uniform flags (USSR) need the scale within a few percent
    let layers: Vec<(f32, RgbImage, Gray)> = [med / 1.05, med, med * 1.05]
        .into_iter()
        .map(|s| {
            let r = resize(hay, (hw / s).round() as u32, (hh / s).round() as u32);
            let g = Gray::from(&r);
            (s, r, g)
        })
        .collect();
    let workers = max_threads();
    std::thread::scope(|sc| {
        let handles: Vec<_> = (0..workers)
            .map(|w| {
                let layers = &layers;
                sc.spawn(move || {
                    let mut found: Vec<(usize, Cand)> = vec![];
                    for (i, t) in templates.iter().enumerate().skip(w).step_by(workers) {
                        let mut mine: Vec<Cand> = vec![];
                        for (s, r, g) in layers {
                            for (x, y) in peaks(g, &t.gray, 0.45) {
                                let c = cand(rgb_at(r, &t.img, x as u32, y as u32), *s, x as u32, y as u32, &t.img);
                                // same flag found at several scales: keep the best one
                                match mine.iter_mut().find(|f| overlap(f, &c) > 0.5) {
                                    Some(f) if f.score < c.score => *f = c,
                                    Some(_) => {}
                                    None => mine.push(c),
                                }
                            }
                        }
                        found.extend(mine.into_iter().map(|c| (i, c)));
                    }
                    found
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    })
}

/// Scanning runs during a battle: at most two threads, so the game keeps its frame rate.
fn max_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 2)
}

/// Local maxima of the luma TM_CCOEFF_NORMED map, at least ~0.7 template widths apart.
fn peaks(hay: &Gray, tpl: &Gray, min: f32) -> Vec<(usize, usize)> {
    let Some((map, mw, _)) = gray_map(hay, tpl) else { return vec![] };
    let mut idx: Vec<usize> = (0..map.len()).filter(|&i| map[i] >= min).collect();
    idx.sort_by(|&a, &b| map[b].total_cmp(&map[a]));
    let min_dx = (tpl.w as f32 * 0.7) as usize;
    let mut out: Vec<(usize, usize)> = vec![];
    for i in idx {
        let (x, y) = (i % mw, i / mw);
        if out.iter().all(|&(px, py)| px.abs_diff(x) >= min_dx || py.abs_diff(y) >= tpl.h) {
            out.push((x, y));
            if out.len() >= 8 {
                break;
            }
        }
    }
    out
}

/// Full luma TM_CCOEFF_NORMED map: (scores, width, height) of valid positions.
fn gray_map(hay: &Gray, tpl: &Gray) -> Option<(Vec<f32>, usize, usize)> {
    let (tw, th) = (tpl.w, tpl.h);
    if tw == 0 || th == 0 || tw > hay.w || th > hay.h {
        return None;
    }
    let n = (tw * th) as f64;
    let mean = tpl.px.iter().map(|&v| v as f64).sum::<f64>() / n;
    let tp: Vec<f32> = tpl.px.iter().map(|&v| (v as f64 - mean) as f32).collect();
    let t2: f64 = tp.iter().map(|&v| (v as f64).powi(2)).sum();
    if t2 < 1e-6 {
        return None;
    }
    let iw = hay.w + 1;
    let mut s1 = vec![0f64; iw * (hay.h + 1)];
    let mut s2 = vec![0f64; iw * (hay.h + 1)];
    for y in 0..hay.h {
        let (mut r1, mut r2) = (0f64, 0f64);
        for x in 0..hay.w {
            let v = hay.px[y * hay.w + x] as f64;
            r1 += v;
            r2 += v * v;
            s1[(y + 1) * iw + x + 1] = s1[y * iw + x + 1] + r1;
            s2[(y + 1) * iw + x + 1] = s2[y * iw + x + 1] + r2;
        }
    }
    let (mw, mh) = (hay.w - tw + 1, hay.h - th + 1);
    let mut map = vec![0f32; mw * mh];
    for y in 0..mh {
        for x in 0..mw {
            let rect = |t: &[f64]| t[(y + th) * iw + x + tw] - t[y * iw + x + tw] - t[(y + th) * iw + x] + t[y * iw + x];
            let sum = rect(&s1);
            let var = rect(&s2) - sum * sum / n;
            if var < 1e-3 {
                continue;
            }
            let mut num = 0f32;
            for ty in 0..th {
                let h = &hay.px[(y + ty) * hay.w + x..(y + ty) * hay.w + x + tw];
                let t = &tp[ty * tw..(ty + 1) * tw];
                num += h.iter().zip(t).map(|(a, b)| a * b).sum::<f32>();
            }
            map[y * mw + x] = (num as f64 / (t2 * var).sqrt()) as f32;
        }
    }
    Some((map, mw, mh))
}

fn cand(score: f32, s: f32, x: u32, y: u32, t: &RgbImage) -> Cand {
    Cand {
        score,
        scale: s,
        x: x as f32 * s,
        y: y as f32 * s,
        w: t.width() as f32 * s,
        h: t.height() as f32 * s,
    }
}

fn overlap(a: &Cand, b: &Cand) -> f32 {
    let ix = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let iy = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    if ix <= 0.0 || iy <= 0.0 {
        return 0.0;
    }
    ix * iy / (a.w * a.h).min(b.w * b.h)
}

fn scan_full(hay: &RgbImage, templates: &[Template], confidence: f32) -> Vec<MatchResult> {
    let cands = find_candidates(hay, templates);
    let mut results: Vec<MatchResult> = cands
        .iter()
        .map(|&(ti, c)| {
            let t = &templates[ti];
            let (x, y) = (c.x.max(0.0) as u32, c.y.max(0.0) as u32);
            let w = (c.w.round() as u32).min(hay.width().saturating_sub(x)).max(1);
            let h = (c.h.round() as u32).min(hay.height().saturating_sub(y)).max(1);
            let crop = image::imageops::crop_imm(hay, x, y, w, h).to_image();
            let color_dist = color_distance(&crop, &t.img);
            let color_ok = c.score >= confidence && color_dist <= MAX_COLOR_DIST && color_check(&crop, &t.name);
            MatchResult {
                name: t.name.clone(),
                score: Some(c.score),
                scale: c.scale,
                pos: (x, y),
                size: (w, h),
                color_ok,
                found: false,
                note: "",
                color_dist,
                alts: vec![],
            }
        })
        .collect();

    // one nation per flag: overlapping hits, the best one wins
    let nms = |pool: &[usize], results: &mut Vec<MatchResult>| {
        let mut order = pool.to_vec();
        order.sort_by(|&a, &b| cands[b].1.score.total_cmp(&cands[a].1.score));
        let mut accepted: Vec<usize> = vec![];
        for i in order {
            // flags in a strip never overlap; a hit straddling a flag and the gap next to it is junk
            if accepted.iter().any(|&j| overlap(&cands[i].1, &cands[j].1) > 0.15) {
                results[i].note = "overlapped";
                continue;
            }
            accepted.push(i);
        }
        accepted
    };
    let mut pool: Vec<usize> = (0..results.len()).filter(|&i| results[i].color_ok).collect();
    let first = nms(&pool, &mut results);
    // all flags in the strip have the same size and stand in one row: drop candidates that don't fit
    // before the final pass, so a shifted look-alike can't push out the real flag
    if first.len() >= 3 {
        let median = |mut v: Vec<f32>| {
            v.sort_by(f32::total_cmp);
            v[v.len() / 2]
        };
        let ms = median(first.iter().map(|&i| cands[i].1.scale).collect());
        let my = median(first.iter().map(|&i| cands[i].1.y).collect());
        pool.retain(|&i| {
            let c = &cands[i].1;
            let ok = (c.scale / ms - 1.0).abs() <= 0.2 && (c.y - my).abs() <= 0.3 * c.h;
            if !ok {
                results[i].note = "wrong_size";
            }
            ok
        });
        for &i in &pool {
            results[i].note = "";
        }
    }
    for i in nms(&pool, &mut results) {
        results[i].found = true;
    }
    // weak candidates only clutter the report
    results.retain(|r| r.found || r.score.is_some_and(|s| s >= confidence));
    results.sort_by_key(|r| (r.pos.0, !r.found));
    results
}

/// Scans a zone: first locate the flag slots, then recognise each one against a short list of
/// templates. Falls back to the full search when no consistent row of slots is found.
pub fn scan(hay: &RgbImage, templates: &[Template], confidence: f32) -> ScanOutput {
    match find_slots(hay) {
        Some(slots) => {
            let results = classify_slots(hay, templates, &slots, confidence);
            ScanOutput { results, slots: slots.iter().map(|s| (s.x, s.y, s.w, s.h)).collect(), fallback: false }
        }
        None => ScanOutput { results: scan_full(hay, templates, confidence), slots: vec![], fallback: true },
    }
}

#[derive(Debug, Clone, Copy)]
struct Slot {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

// template aspect ratio (40x26 at 1440p)
const FLAG_ASPECT: f32 = 40.0 / 26.0;

fn median_u8(hist: &[u32; 256], total: u32) -> u8 {
    let mut acc = 0;
    for (i, &c) in hist.iter().enumerate() {
        acc += c;
        if acc * 2 >= total {
            return i as u8;
        }
    }
    255
}

/// The fast path can be used on this image (a consistent row of flag places was found).
pub fn has_slots(hay: &RgbImage) -> bool {
    find_slots(hay).is_some()
}

/// Flags are same-size coloured boxes in one row on a dark background with gaps between them.
fn find_slots(hay: &RgbImage) -> Option<Vec<Slot>> {
    let (w, h) = (hay.width(), hay.height());
    if w < 8 || h < 6 {
        return None;
    }
    // background = per-channel median (flags cover well under half of a normal zone)
    let mut hist = [[0u32; 256]; 3];
    for p in hay.pixels() {
        for c in 0..3 {
            hist[c][p[c] as usize] += 1;
        }
    }
    let bg: [u8; 3] = std::array::from_fn(|c| median_u8(&hist[c], w * h));
    let mask: Vec<bool> = hay
        .pixels()
        .map(|p| {
            let (_, s, v) = rgb_to_hsv(p[0], p[1], p[2]);
            // saturated or bright, or (dark flag parts like the FRG black) clearly off the background
            (s > 110 && v > 70) || v > 225 || (0..3).map(|c| p[c].abs_diff(bg[c])).max().unwrap_or(0) > 40
        })
        .collect();
    let at = |x: u32, y: u32| mask[(y * w + x) as usize];
    let cols: Vec<u32> = (0..w).map(|x| (0..h).filter(|&y| at(x, y)).count() as u32).collect();
    let cmax = *cols.iter().max()?;
    if cmax == 0 {
        return None;
    }
    let mut runs: Vec<(u32, u32)> = vec![];
    let mut x = 0;
    while x < w {
        if cols[x as usize] * 2 >= cmax {
            let start = x;
            while x < w && cols[x as usize] * 2 >= cmax {
                x += 1;
            }
            runs.push((start, x));
        } else {
            x += 1;
        }
    }
    // common row: rows that are flag-like across the flag columns
    let rows: Vec<u32> = (0..h)
        .map(|y| runs.iter().map(|&(a, b)| (a..b).filter(|&x| at(x, y)).count() as u32).sum())
        .collect();
    let rmax = *rows.iter().max()?;
    let (mut top, mut bottom, mut best) = (0, 0, 0);
    let mut y = 0;
    while y < h {
        if rows[y as usize] * 2 >= rmax {
            let start = y;
            while y < h && rows[y as usize] * 2 >= rmax {
                y += 1;
            }
            if y - start > best {
                (top, bottom, best) = (start, y, y - start);
            }
        } else {
            y += 1;
        }
    }
    let fh = bottom - top;
    // a real row of flags has background above and below; a band touching both edges is
    // background texture (or a zone cut tight around the flags): leave it to the full search
    if fh < 6 || (top == 0 && bottom == h) {
        return None;
    }
    let fw = fh as f32 * FLAG_ASPECT;
    // dark details inside a flag (a black cross at small scale) cut a run in two; real gaps
    // between flags are ~0.18 flag widths, so bridging up to 0.1 is safe
    let bridge = (0.1 * fw).max(2.0) as u32;
    let mut merged: Vec<(u32, u32)> = vec![];
    for (a, b) in runs {
        match merged.last_mut() {
            Some(last) if a - last.1 <= bridge => last.1 = b,
            _ => merged.push((a, b)),
        }
    }
    let mut slots = vec![];
    for (a, b) in merged {
        let rw = (b - a) as f32;
        if rw < 0.5 * fw {
            continue;
        }
        // flags touching each other: cut the run into equal parts; a run wider than three flags is
        // not a row of flags (a textured background, grass), leave it to the full search
        let k = (rw / fw).round().max(1.0) as u32;
        if k > 3 {
            continue;
        }
        let part = rw / k as f32;
        for i in 0..k {
            slots.push(Slot { x: a + (i as f32 * part) as u32, y: top, w: part as u32, h: fh });
        }
    }
    slots.retain(|s| (s.w as f32 / fw - 1.0).abs() <= 0.25);
    if slots.len() < 2 {
        return None;
    }
    let mut ws: Vec<u32> = slots.iter().map(|s| s.w).collect();
    ws.sort_unstable();
    let mw = ws[ws.len() / 2] as f32;
    slots.retain(|s| (s.w as f32 / mw - 1.0).abs() <= 0.15);
    (slots.len() >= 2).then_some(slots)
}

fn crop_gray(g: &Gray, x: usize, y: usize, w: usize, h: usize) -> Gray {
    let (w, h) = (w.min(g.w - x.min(g.w)), h.min(g.h - y.min(g.h)));
    let mut px = Vec::with_capacity(w * h);
    for yy in y..y + h {
        px.extend_from_slice(&g.px[yy * g.w + x..yy * g.w + x + w]);
    }
    Gray { w, h, px }
}

/// Recognises each slot: shortlist by colour fingerprint, then TM_CCOEFF_NORMED in a small window
/// around the slot at three neighbouring scales, then the colour checks.
/// How far under the threshold a slot match may be, and how far ahead of any other flag it must be.
const SLOT_SLACK: f32 = 0.15;
const SLOT_LEAD: f32 = 0.30;

fn classify_slots(hay: &RgbImage, templates: &[Template], slots: &[Slot], confidence: f32) -> Vec<MatchResult> {
    const SHORTLIST: usize = 8;
    const MARGIN: u32 = 4;
    if templates.is_empty() {
        return vec![];
    }
    let mut ths: Vec<u32> = templates.iter().map(|t| t.img.height()).collect();
    ths.sort_unstable();
    let mut hs: Vec<u32> = slots.iter().map(|s| s.h).collect();
    hs.sort_unstable();
    let s0 = hs[hs.len() / 2] as f32 / ths[ths.len() / 2] as f32;
    let (hw, hh) = (hay.width() as f32, hay.height() as f32);
    let layers: Vec<(f32, RgbImage, Gray)> = [s0 / 1.05, s0, s0 * 1.05]
        .into_iter()
        .map(|s| {
            let r = resize(hay, (hw / s).round() as u32, (hh / s).round() as u32);
            let g = Gray::from(&r);
            (s, r, g)
        })
        .collect();

    let mut out = vec![];
    for slot in slots {
        let crop = image::imageops::crop_imm(hay, slot.x, slot.y, slot.w, slot.h).to_image();
        let sig = signature(&crop);
        let mut order: Vec<usize> = (0..templates.len()).collect();
        order.sort_by(|&a, &b| sig_dist(&sig, &templates[a].sig).total_cmp(&sig_dist(&sig, &templates[b].sig)));
        order.truncate(SHORTLIST);

        // (template, score, scale, x, y) in original coordinates
        let mut cands: Vec<(usize, f32, f32, f32, f32)> = vec![];
        for &ti in &order {
            let t = &templates[ti];
            let mut best: Option<(f32, f32, f32, f32)> = None;
            for (s, r, g) in &layers {
                let lx = ((slot.x as f32 / s) as u32).saturating_sub(MARGIN);
                let ly = ((slot.y as f32 / s) as u32).saturating_sub(MARGIN);
                let lw = t.img.width() + 2 * MARGIN;
                let lh = t.img.height() + 2 * MARGIN;
                let sub = crop_gray(g, lx as usize, ly as usize, lw as usize, lh as usize);
                let Some((_, x, y)) = gray_best(&sub, &t.gray, None) else { continue };
                let (px, py) = (lx + x as u32, ly + y as u32);
                if px + t.img.width() > r.width() || py + t.img.height() > r.height() {
                    continue;
                }
                let score = rgb_at(r, &t.img, px, py);
                if best.is_none_or(|b| score > b.0) {
                    best = Some((score, *s, px as f32 * s, py as f32 * s));
                }
            }
            if let Some((score, s, x, y)) = best {
                cands.push((ti, score, s, x, y));
            }
        }
        cands.sort_by(|a, b| b.1.total_cmp(&a.1));

        // A place is known to hold a flag, so a score a bit under the threshold still counts when
        // no other flag comes close. Upscaled captures blur small emblems (USSR, China) to ~0.7.
        let runner_up = |ti: usize| {
            let me = crate::vehicles::canon_flag(&templates[ti].name);
            cands
                .iter()
                .filter(|c| crate::vehicles::canon_flag(&templates[c.0].name) != me)
                .map(|c| c.1)
                .fold(0.0_f32, f32::max)
        };
        let check = |&(ti, score, s, x, y): &(usize, f32, f32, f32, f32)| {
            let t = &templates[ti];
            let confident = score >= confidence || (score >= confidence - SLOT_SLACK && score - runner_up(ti) >= SLOT_LEAD);
            let (x, y) = (x.max(0.0) as u32, y.max(0.0) as u32);
            let w = ((t.img.width() as f32 * s).round() as u32).min(hay.width().saturating_sub(x)).max(1);
            let h = ((t.img.height() as f32 * s).round() as u32).min(hay.height().saturating_sub(y)).max(1);
            let crop = image::imageops::crop_imm(hay, x, y, w, h).to_image();
            let dist = color_distance(&crop, &t.img);
            let ok = confident && dist <= MAX_COLOR_DIST && color_check(&crop, &t.name);
            (ok, dist, (x, y), (w, h))
        };
        let winner = cands.iter().position(|c| check(c).0);
        let pick = winner.unwrap_or(0);
        let Some(&c) = cands.get(pick) else { continue };
        let (ok, dist, pos, size) = check(&c);
        let alts = cands
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != pick)
            .take(3)
            .map(|(_, &(ti, sc, ..))| (templates[ti].name.clone(), sc))
            .collect();
        out.push(MatchResult {
            name: templates[c.0].name.clone(),
            score: Some(c.1),
            scale: c.2,
            pos,
            size,
            color_ok: ok,
            found: winner.is_some(),
            note: if winner.is_some() { "" } else { "unrecognized" },
            color_dist: dist,
            alts,
        });
    }
    out
}

pub fn describe(r: &MatchResult, confidence: f32) -> String {
    let line = describe_main(r, confidence);
    if r.alts.is_empty() {
        return line;
    }
    let list: Vec<String> = r.alts.iter().map(|(n, s)| format!("{n} {s:.2}")).collect();
    format!("{line}  {}", crate::t!("report.next", list = list.join(", ")))
}

fn describe_main(r: &MatchResult, confidence: f32) -> String {
    use crate::t;
    let at = format!("x={} y={} ×{:.2}", r.pos.0, r.pos.1, r.scale);
    let name = &r.name;
    let dist = format!("{:.0}", r.color_dist);
    match r.score {
        None => t!("report.too_small", name = name),
        Some(s) => {
            let score = format!("{s:.2}");
            if r.note == "unrecognized" {
                t!("report.unrecognized", name = name, score = score, at = at)
            } else if r.found {
                t!("report.found", name = name, score = score, dist = dist, at = at)
            } else if s >= confidence && r.note == "overlapped" {
                t!("report.overlapped", name = name, score = score, at = at)
            } else if s >= confidence && r.note == "wrong_size" {
                t!("report.wrong_size", name = name, score = score, at = at)
            } else if s >= confidence && r.color_dist > MAX_COLOR_DIST {
                t!("report.color_mismatch", name = name, score = score, dist = dist, at = at)
            } else if s >= confidence {
                t!("report.color_filter", name = name, score = score, at = at)
            } else {
                t!("report.none", name = name, score = score)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_matches_opencv() {
        assert_eq!(rgb_to_hsv(255, 0, 0), (0, 255, 255));
        assert_eq!(rgb_to_hsv(0, 255, 0), (60, 255, 255));
        assert_eq!(rgb_to_hsv(0, 0, 255), (120, 255, 255));
        assert_eq!(rgb_to_hsv(0, 0, 0), (0, 0, 0));
    }

    #[test]
    fn finds_embedded_flags_in_synthetic_strip() {
        let tpls = load_templates(Path::new("/nonexistent"));
        assert!(tpls.len() >= 60, "{}", tpls.len());
        let mut hay = RgbImage::from_pixel(240, 34, image::Rgb([30, 30, 34]));
        let usa = tpls.iter().find(|t| t.name == "usa").unwrap();
        let ger = tpls.iter().find(|t| t.name == "germany").unwrap();
        image::imageops::overlay(&mut hay, &usa.img, 17, 3);
        image::imageops::overlay(&mut hay, &ger.img, 120, 5);
        let res = scan(&hay, &tpls, 0.70).results;
        let found: Vec<_> = res.iter().filter(|r| r.found).map(|r| r.name.as_str()).collect();
        assert!(found.contains(&"usa"), "{res:?}");
        assert!(found.contains(&"germany"), "{res:?}");
        let usa_r = res.iter().find(|r| r.name == "usa").unwrap();
        assert!(usa_r.score.unwrap() > 0.95, "{usa_r:?}");
        assert_eq!(found.len(), 2, "extra nations: {res:?}");
    }

    #[test]
    fn no_flags_on_grass() {
        let tpls = load_templates(Path::new("/nonexistent"));
        let hay = RgbImage::from_fn(783, 58, |x, y| {
            let v = ((x as f32 * 0.37).sin() * 18.0 + (y as f32 * 0.9).cos() * 8.0) as i32;
            image::Rgb([(120 + v) as u8, (190 + v) as u8, (90 + v / 2) as u8])
        });
        let res = scan(&hay, &tpls, 0.70).results;
        assert!(res.iter().all(|r| !r.found), "{:#?}", res.iter().map(|r| describe(r, 0.7)).collect::<Vec<_>>());
    }

    #[test]
    fn finds_scaled_flags_one_nation_per_flag() {
        let tpls = load_templates(Path::new("/nonexistent"));
        let names = ["france", "italy", "ussr", "sweden", "britain"];
        let mut hay = RgbImage::from_pixel(560, 62, image::Rgb([28, 30, 36]));
        for (i, n) in names.iter().enumerate() {
            let t = &tpls.iter().find(|t| t.name == *n).unwrap().img;
            let big = image::imageops::resize(t, 62, 40, image::imageops::FilterType::Triangle);
            image::imageops::overlay(&mut hay, &big, 12 + i as i64 * 105, 11);
        }
        let res = scan(&hay, &tpls, 0.70).results;
        let mut found: Vec<_> = res.iter().filter(|r| r.found).map(|r| r.name.as_str()).collect();
        found.sort();
        let mut want = names.to_vec();
        want.sort();
        assert_eq!(found, want, "{:#?}", res.iter().map(|r| describe(r, 0.7)).collect::<Vec<_>>());
    }
}
