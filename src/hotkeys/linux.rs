use super::{poll_loop, Bindings, HotkeyEvent};
use std::sync::mpsc::Sender;

pub fn start(bindings: Bindings, tx: Sender<HotkeyEvent>) -> String {
    let keyboards = evdev_keyboards();
    if !keyboards.is_empty() {
        let n = keyboards.len();
        let (b, t) = (bindings.clone(), tx.clone());
        std::thread::Builder::new()
            .name("hotkeys-evdev".into())
            .spawn(move || evdev_loop(keyboards, b, t))
            .ok();
        return crate::t!("hotkeys.evdev", count = n);
    }
    log::warn!("Нет доступа к /dev/input (добавь пользователя в группу input) — использую X11/portal");

    let mut backends = Vec::new();
    if std::env::var_os("DISPLAY").is_some() {
        if let Ok((conn, _)) = x11rb::connect(None) {
            let (b, t) = (bindings.clone(), tx.clone());
            std::thread::Builder::new()
                .name("hotkeys-x11".into())
                .spawn(move || x11_loop(conn, b, t))
                .ok();
            backends.push("X11");
        }
    }
    #[cfg(feature = "portal")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let (b, t) = (bindings.clone(), tx.clone());
        std::thread::Builder::new()
            .name("hotkeys-portal".into())
            .spawn(move || {
                if let Err(e) = portal_loop(b, t) {
                    log::warn!("xdg-portal GlobalShortcuts недоступен: {e}");
                }
            })
            .ok();
        backends.push("xdg-portal");
    }
    if backends.is_empty() {
        crate::t!("hotkeys.none")
    } else {
        backends.join(" + ")
    }
}

fn evdev_keyboards() -> Vec<evdev::Device> {
    evdev::enumerate()
        .map(|(_, d)| d)
        .filter(|d| {
            d.supported_keys().is_some_and(|k| {
                k.contains(evdev::Key::KEY_A) && k.contains(evdev::Key::KEY_SPACE)
            })
        })
        .collect()
}

fn evdev_loop(mut devs: Vec<evdev::Device>, bindings: Bindings, tx: Sender<HotkeyEvent>) {
    poll_loop(bindings, tx, |defs| {
        let mut out = vec![false; defs.len()];
        devs.retain(|d| match d.get_key_state() {
            Ok(state) => {
                for (o, k) in out.iter_mut().zip(defs) {
                    *o |= state.contains(evdev::Key::new(k.evdev));
                }
                true
            }
            // unplugged
            Err(_) => false,
        });
        out
    });
}

fn x11_loop(conn: x11rb::rust_connection::RustConnection, bindings: Bindings, tx: Sender<HotkeyEvent>) {
    use x11rb::protocol::xproto::ConnectionExt;
    poll_loop(bindings, tx, |defs| {
        let Ok(keys) = conn.query_keymap().map_err(|_| ()).and_then(|c| c.reply().map_err(|_| ())) else {
            return vec![false; defs.len()];
        };
        defs.iter()
            .map(|k| {
                let kc = k.evdev as usize + 8;
                kc < 256 && keys.keys[kc / 8] & (1 << (kc % 8)) != 0
            })
            .collect()
    });
}

#[cfg(feature = "portal")]
fn portal_loop(bindings: Bindings, tx: Sender<HotkeyEvent>) -> Result<(), String> {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use futures_util::StreamExt;

    let keys = *bindings.lock().unwrap_or_else(|e| e.into_inner());
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    rt.block_on(async move {
        let gs = GlobalShortcuts::new().await.map_err(|e| e.to_string())?;
        let session = gs.create_session().await.map_err(|e| e.to_string())?;
        let shortcuts = [
            NewShortcut::new("scan", crate::t!("hotkeys.portal_scan").as_str()).preferred_trigger(keys[0].map(|k| k.keysym)),
            NewShortcut::new("toggle_overlay", crate::t!("hotkeys.portal_toggle").as_str())
                .preferred_trigger(keys[1].map(|k| k.keysym)),
        ];
        gs.bind_shortcuts(&session, &shortcuts, None)
            .await
            .map_err(|e| e.to_string())?
            .response()
            .map_err(|e| e.to_string())?;
        log::info!("xdg-portal GlobalShortcuts: хоткеи зарегистрированы (переназначаются в настройках системы)");
        let mut stream = gs.receive_activated().await.map_err(|e| e.to_string())?;
        while let Some(act) = stream.next().await {
            let ev = match act.shortcut_id() {
                "scan" => HotkeyEvent::Scan,
                "toggle_overlay" => HotkeyEvent::ToggleOverlay,
                _ => continue,
            };
            if tx.send(ev).is_err() {
                break;
            }
        }
        drop(session);
        Ok(())
    })
}
