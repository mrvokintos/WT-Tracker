# config.json

Файл создаётся и обновляется трекером, ручное редактирование обычно не требуется. Конфигурация Python-версии читается без изменений.

```json
{
  "overlay":  { "x": 30, "y": 150, "size": 20, "opacity": 100, "gamescope": true },
  "filters":  { "nations": ["Germany", "USSR"], "types": ["tank", "plane", "heli", "zsu", "special"] },
  "scanner":  { "region": [x, y, w, h], "confidence_threshold": 0.7 },
  "hotkeys":  { "scan": "\\", "overlay_toggle": "f5" },
  "display":  { "show_notes": true, "show_names": true },
  "audio":    { "enabled": true },
  "record":   { "enabled": false },
  "ui":       { "language": "auto", "backend": "x11" },
  "player":   { "nickname": "" },
  "game":     { "dir": "" },
  "db":       { "auto_update": true, "url": "https://github.com/mrvokintos/WT-Vehicles-Parser/releases/download/db-latest" }
}
```

- `overlay.x/y`: позиция оверлея; при запуске через gamescope — в пикселях игры. `opacity`: от 10 до 100. `gamescope: false` отключает вывод оверлея внутри gamescope.
- `filters.nations`: нации противника, заполняются при сканировании флагов.
- `scanner.region`: зона сканирования в пикселях рабочего стола, задаётся кнопкой «Выбрать зону». `confidence_threshold`: порог совпадения; более высокое значение снижает число ложных срабатываний, но повышает вероятность пропустить флаг.
- `hotkeys`: одиночные клавиши без модификаторов, например `a`, `f5`, `\`, `/`, `insert`, `page up`, `num 5`.
- `record.enabled`: сохранение необработанного киллфида в `recordings/`.
- `player.nickname`: ваш ник в игре без клан-тега. Точка отсчёта для определения сторон в смешанных боях; регистр учитывается.
- `game.dir`: папка War Thunder, если трекер не нашёл её сам. Нужна только для однократного чтения шрифта со значками техники; дальше он хранится в рабочей папке как `symbols_skyquake.ttf`.
- `ui.language`: язык интерфейса: `auto` (по системе) или код перевода из `lang/` (`en`, `ru`).
- `ui.backend` (Linux): режим окна при запуске. `x11` (по умолчанию) — через X11/XWayland, так оверлей держится поверх игры; `wayland` — нативное Wayland-окно, оверлей может не оставаться поверх. Флаги `--x11` и `--wayland` важнее этой настройки. Применяется после перезапуска.
- `db`: автообновление базы техники и адрес релиза.

Поля `monitor_idx` и `scanner.hotkey` сохранены для совместимости и не используются.
