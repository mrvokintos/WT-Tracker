use crate::logic::SoundKind;
use crate::paths;
use std::io::Cursor;
use std::sync::mpsc::{channel, Sender};

pub fn spawn() -> Sender<SoundKind> {
    let (tx, rx) = channel::<SoundKind>();
    std::thread::Builder::new()
        .name("audio".into())
        .spawn(move || {
            let (_stream, handle) = match rodio::OutputStream::try_default() {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("Звук недоступен: {e}");
                    for _ in rx {}
                    return;
                }
            };
            let sink = match rodio::Sink::try_new(&handle) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("Ошибка аудио: {e}");
                    for _ in rx {}
                    return;
                }
            };
            for kind in rx {
                let bytes = match kind {
                    SoundKind::Enemy => paths::read_or_embedded(paths::SOUND_ENEMY, paths::EMBEDDED_SOUND_ENEMY),
                    SoundKind::Ally => paths::read_or_embedded(paths::SOUND_ALLY, paths::EMBEDDED_SOUND_ALLY),
                };
                let source = match rodio::Decoder::new(Cursor::new(bytes)) {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!("Не удалось декодировать звук: {e}");
                        continue;
                    }
                };
                // queued, so an enemy and an ally alert at the same moment both play
                sink.append(source);
            }
        })
        .expect("audio thread");
    tx
}
