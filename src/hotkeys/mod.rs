pub mod keys;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

use keys::KeyDef;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyEvent {
    Scan,
    ToggleOverlay,
}

pub const EVENTS: [HotkeyEvent; 2] = [HotkeyEvent::Scan, HotkeyEvent::ToggleOverlay];

pub type Bindings = Arc<Mutex<[Option<&'static KeyDef>; 2]>>;

pub fn bindings(scan: &str, overlay: &str) -> Bindings {
    Arc::new(Mutex::new([keys::parse(scan), keys::parse(overlay)]))
}

pub fn start(bindings: Bindings, tx: Sender<HotkeyEvent>) -> String {
    #[cfg(target_os = "linux")]
    {
        linux::start(bindings, tx)
    }
    #[cfg(windows)]
    {
        windows::start(bindings, tx)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (bindings, tx);
        crate::t!("hotkeys.unsupported")
    }
}

const CHAT_TIMEOUT: Duration = Duration::from_secs(20);

pub(crate) fn poll_loop(
    bindings: Bindings,
    tx: Sender<HotkeyEvent>,
    mut is_pressed: impl FnMut(&[&'static KeyDef]) -> Vec<bool>,
) {
    let enter = keys::parse("enter").expect("enter");
    let esc = keys::parse("esc").expect("esc");
    let mut was = [false; 4];
    // Enter opens/sends the in-game chat, Esc closes it; text keys typed there aren't hotkeys
    let mut chat_since: Option<Instant> = None;
    loop {
        let keys = *bindings.lock().unwrap_or_else(|e| e.into_inner());
        let slots = [keys[0], keys[1], Some(enter), Some(esc)];
        let defs: Vec<&'static KeyDef> = slots.iter().flatten().copied().collect();
        let states = is_pressed(&defs);
        let mut di = 0;
        for (i, k) in slots.iter().enumerate() {
            let down = k.is_some() && {
                di += 1;
                states.get(di - 1).copied().unwrap_or(false)
            };
            let pressed = down && !was[i];
            was[i] = down;
            if !pressed {
                continue;
            }
            match i {
                2 => chat_since = if chat_since.is_some() { None } else { Some(Instant::now()) },
                3 => chat_since = None,
                _ => {
                    let chat_open = chat_since.is_some_and(|t| t.elapsed() < CHAT_TIMEOUT);
                    if chat_open && k.is_some_and(|k| k.types_text()) {
                        continue;
                    }
                    if tx.send(EVENTS[i]).is_err() {
                        return;
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}
