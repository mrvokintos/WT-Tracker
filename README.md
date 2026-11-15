# WT Tracker

**English** | [Русский](README.ru.md)

An overlay for War Thunder. It lists enemy vehicles from the kill feed, plays an alert when a player is close to a nuke, and detects enemy nations from the flags on the scoreboard (Tab).

Supports Linux (Wayland and X11, including games running in gamescope) and Windows. The interface is available in English and Russian; the language follows the system and can be changed on the Settings tab.

Inspired by [WT Tracker by Allotron](https://github.com/Allotron/WarThunder_3_programs).

## Installation

Download the archive for your system from the [Releases](https://github.com/mrvokintos/WT-Tracker/releases) page, unpack it and run the executable. The "Create shortcut" button on the Settings tab adds the tracker to the application menu (`~/.local/share/applications` on Linux, the Start menu on Windows); if you move the folder later, press it again. Please mention the version (shown in the window title) in bug reports.

Upgrading from the Python version into the same folder: delete the old `vehicles.json` (and `flags/`, unless you made your own templates). They would override the built-in ones; the tracker warns about an old database in the log and at the bottom of the window. The tracker connects once the game opens its local API (`localhost:8111`).

Building from source:

```bash
cargo build --release
```

Linux builds need the development packages for xkbcommon, wayland, alsa, dbus and X11. On Bazzite and Silverblue, building inside distrobox is the easiest option.

## Usage

- `\` scans the flags. Hold Tab during a battle and press the key; the enemy nations are selected automatically.
- `F5` shows or hides the overlay.

The window has two tabs: **Tracker** (nickname, nation and vehicle type filters, database) and **Settings** (overlay, hotkeys, scan zone, language and other program options). Both keys can be changed on the Settings tab. While the in-game chat is open (Enter), keys that type characters (such as `\`) are not treated as hotkeys. The chat state is guessed from Enter and Esc presses, so an Enter pressed in a menu or on the respawn screen can make the scan key ignore presses for up to 20 seconds. Pressing Esc resets it.

The kill feed is parsed in all 21 game client languages.

The scan zone only needs to be set once. Click "Select zone" on the Settings tab, then hold Tab in a battle and press `\`. A screenshot opens: zoom in with the mouse wheel, select the flag strip with the left button and press Enter. If the zone covers both teams, the enemy side (right) is detected automatically. The flag strip gets longer with the number of nations; when the flags run into a side of the zone, the tracker widens it on that side, so the zone does not need to be redrawn for bigger battles. Flag scale adapts to any resolution. The zone picker can also be opened with `--pick-region`.

The scoreboard shows the flag of the vehicle's operator country, not its tech tree: a Russian tricolour for a T-90M from the USSR tree, the West German flag for a Leopard 2 from the German tree, Australia for an M1A1 AIM from the US tree. The tracker knows every operator flag the game uses and maps each one to the tech trees it appears in, based on the vehicle database.

### Telling the teams apart

In mixed battles the same tech tree is often on both teams, so a nation alone doesn't say who is an enemy. The tracker works it out from the kill feed: every kill joins players of different teams, so it two-colours the kill graph starting from known points:

- **you**, if your nickname is set in the settings (without the clan tag);
- **vehicles whose flag is on one side only**, according to the last scan of both teams (for example a Swedish flag only on the enemy side);
- then the graph: whoever an enemy killed is an ally, whoever an ally killed is an enemy.

Players whose side isn't known yet are shown with "?" if their tree is on the enemy team. Allies are not shown. Setting your nickname makes this work from your first kill, even without a scan. Without a scan of both teams and without a nickname the tracker falls back to the selected nations, as before.

While the Settings tab is open, the overlay can be dragged with the mouse. When the game runs in gamescope, use the minimap below the X/Y fields instead.

## Linux notes

- Hotkeys are read from `/dev/input`, which requires membership in the `input` group. Without it the tracker falls back to X11 and xdg-portal, which do not work in every environment. Note that members of `input` can read input from all keyboards.
- The window runs through X11/XWayland by default, so the overlay can stay above the game. A native Wayland window can be chosen under Settings → Program → Window mode (applies after a restart); the `--x11` and `--wayland` flags override that setting for a single run.
- Borderless windowed mode is recommended. In exclusive fullscreen KWin places the game above the overlay.
- If the game runs through gamescope, the tracker detects it and draws the overlay inside gamescope.
- Screenshots for scanning are taken with spectacle, grim, xdg-portal or X11, depending on availability.

### The input group

On most distributions:

```bash
sudo usermod -aG input $USER
```

On Fedora Atomic (Silverblue, Kinoite, Bazzite) the group is defined in `/usr/lib/group` and `usermod` cannot find it. Copy it to `/etc/group` first:

```bash
grep -E '^input:' /usr/lib/group | sudo tee -a /etc/group
sudo usermod -aG input $USER
```

Log out and back in afterwards. To check: `id -nG | grep -w input`.

## Windows

In exclusive fullscreen the overlay is not visible above the game and screen captures come out black. Run the game in borderless windowed mode. If a scan gets a black frame, the tracker reports it in the overlay.

## Vehicle database

Vehicle names match the kill feed exactly. The database is built by [WT-Vehicles-Parser](https://github.com/mrvokintos/WT-Vehicles-Parser) from the [datamine](https://github.com/gszabi99/War-Thunder-Datamine), and CI publishes a new version after each game patch. The tracker checks for updates on startup; the "Update" button at the bottom of the window forces a download. Without network access the built-in database is used.

Vehicles missing from the database (for example right after a patch) are logged to `unknown.txt`. The side of such a player is inferred from who they destroyed; if both vehicles are unknown, the player is shown in the overlay marked with "?".

## Network access

- `http://localhost:8111`: the game's local API, polled continuously.
- `https://github.com/mrvokintos/WT-Vehicles-Parser/releases/download/db-latest/manifest.json`: on startup when database auto-update is enabled. `vehicles.json` is downloaded from the same place only when a new version is available.

The tracker makes no other requests. Auto-update can be turned off with the "auto" checkbox at the bottom of the window or `db.auto_update` in `config.json`.

## Files

The configuration (`config.json`, documented in [docs/config.md](docs/config.md)), `unknown.txt` and other working files are stored next to the executable. If they are not there, `~/.config/wt-tracker` (Linux) or `%APPDATA%\WT Tracker` (Windows) is used. The location can be overridden with `WT_TRACKER_DIR`.

Vehicle markers in names (◥ ◍ ▄ ☢ ...) are icons from the game's own symbol font. The font is not distributed with the tracker: on first start it is read from the local War Thunder install (found automatically, or set `game.dir` in `config.json`) and kept in the working folder as `symbols_skyquake.ttf`. Without the game the markers are drawn with system fonts and look different. Delete that file to pick up a new version after a game update. CJK nicknames use system fonts (Noto Sans CJK on Linux, Microsoft YaHei / Yu Gothic / Malgun Gothic on Windows).

`flags/`, `vehicles.json`, the sounds and the translations are embedded in the executable. Files with the same names next to it take precedence, so only put them there on purpose.

## Contributing

Pull requests are welcome, especially:

- **Translations.** The interface strings live in `lang/<code>.json`; `lang/en.json` is the reference. Copy it (release archives contain the same files in `lang-templates/`), translate the values and keep the `{placeholders}` intact. `cargo test` checks that a translation has every key and the same placeholders. To try a translation without rebuilding, put the file into a `lang/` folder next to the executable; it only overrides the keys it contains.
- **Sounds.** The nuke alerts (`alert.mp3`, `alert_ally.mp3`) are the same for every language. Better or language-specific sounds are welcome; you can also replace them locally by putting files with the same names next to the executable.
- **Kill feed samples from other clients.** The kill feed parser is built from the game's own strings for all 21 client languages, but only the Russian client has been checked against real battles. Battle recordings ("Record battle log") from Japanese, Korean and other clients help a lot.
- **Flag templates.** `flags/<code>.png` are rendered from the game's own flag icons by `tools/update_flags.py` (needs `rsvg-convert`); rerun it when a patch adds a new operator country. Any PNG in `flags/` is embedded automatically.

Not localized yet: log output, command-line help and the parser's messages are in Russian. PRs translating them are welcome too.

## Disclaimer

WT Tracker does not modify game files or memory and does not inject anything into the game process. It only reads the game's local API at `localhost:8111`, takes screenshots of the selected zone for flag detection and draws its own window on top of the game.

## Debugging

```bash
RUST_LOG=debug wt-tracker
wt-tracker --calibrate shot.png [--flags dir] [--conf 0.7]   # test the flag scan on an image
python3 tools/mock_api.py 18111 &                            # simulated battle, no game needed
WT_API_URL=http://localhost:18111 wt-tracker
```

After each scan, `last_scan_debug.png` (the zone contents) and `last_scan_report.txt` are written to the tracker folder.

The "Record battle log" option saves the raw kill feed to `recordings/`. The database can be checked against these recordings with `wt-vehicles-parser verify recordings`.

## License

[GPL-3.0](LICENSE)
