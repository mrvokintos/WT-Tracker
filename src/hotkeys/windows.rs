use super::{poll_loop, Bindings, HotkeyEvent};
use std::sync::mpsc::Sender;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

pub fn start(bindings: Bindings, tx: Sender<HotkeyEvent>) -> String {
    std::thread::Builder::new()
        .name("hotkeys-win".into())
        .spawn(move || {
            poll_loop(bindings, tx, |defs| {
                defs.iter()
                    .map(|k| unsafe { GetAsyncKeyState(k.vk as i32) } as u16 & 0x8000 != 0)
                    .collect()
            })
        })
        .ok();
    "GetAsyncKeyState".into()
}
