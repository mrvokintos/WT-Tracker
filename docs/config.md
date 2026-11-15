# config.json

The file is created and updated by the tracker; editing it by hand is usually not needed. Configuration files from the Python version are read as is.

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

- `overlay.x/y`: overlay position; under gamescope, in game pixels. `opacity`: 10 to 100. `gamescope: false` disables drawing the overlay inside gamescope.
- `filters.nations`: enemy nations, filled in by the flag scan.
- `scanner.region`: scan area in desktop pixels, set with the "Pick area" button. `confidence_threshold`: match threshold; a higher value gives fewer false positives but makes missing a flag more likely.
- `hotkeys`: single keys without modifiers, e.g. `a`, `f5`, `\`, `/`, `insert`, `page up`, `num 5`.
- `record.enabled`: save the raw kill feed to `recordings/`.
- `player.nickname`: your in-game name without the clan tag. Used as the reference point for telling sides apart in mixed battles; case-sensitive.
- `game.dir`: War Thunder folder, if the tracker can't find it. Only used to read the game's vehicle-marker font once; it is then kept as `symbols_skyquake.ttf` in the working folder.
- `ui.language`: interface language: `auto` (system) or a translation code from `lang/` (`en`, `ru`).
- `ui.backend` (Linux): window mode on startup. `x11` (default) runs through X11/XWayland, which keeps the overlay above the game; `wayland` uses a native Wayland window, where the overlay may not stay on top. The `--x11` and `--wayland` command-line flags override it. Applies after a restart.
- `db`: vehicle database auto-update and the release URL.

`monitor_idx` and `scanner.hotkey` are kept for compatibility and are not used.
