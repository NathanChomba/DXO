# DXO roadmap

Follows the product roadmap (DXO-Roadmap.pdf, 4 Nov 2026). Status key:

* **built** = written, unit-tested where it is engine code; **needs a first
  run on Windows** unless marked tested
* **hw** = also needs hardware that hasn't been on the desk yet
* **not built** = with the reason

The engine (Rust) is tested on every build: `cargo test --workspace`
(140 tests). The app (WinUI) has no automated tests; each page needs a
click-through after a build.

## Shipped in v0.2

Devices (DualSense, Edge, DS4, Switch Pro, generic), up to 4 pads with stable
identity; virtual Xbox 360 / DS4 via ViGEm + HidHide; remaps, keys, mouse,
turbo, toggle, macros, shift layer; gyro-to-mouse, CemuHook; touchpad/stick as
mouse; lightbar, rumble, adaptive triggers, controller audio; per-game
profiles; controller navigation; system chords; tray, installer, diagnostics.

## 1 · Controller-first polish

| Item | Status | Where |
|---|---|---|
| NumberBox adjust with the pad | built | `MainWindow.AdjustNumberBox`: Left/Right step it, Up/Down move on |
| Focusable Profiles-strip pills | built | pills are buttons; landing focus on one selects that controller |
| "Nothing focused yet" cue | built | `NavCue` hint while a pad is connected and nothing is highlighted |
| First-press focus polish | built | each page (and Profiles tab) remembers where focus was |

## 2 · Mapping depth

| Item | Status | Where |
|---|---|---|
| Dual-stage triggers | built | sources `l2_soft` / `l2_full` / `r2_soft` / `r2_full` in `bindings.rs`; rows on the Buttons tab |
| Tap / hold / double-tap | built | `Binding.hold`, `Binding.double_tap`; Press / Hold / Double-tap bar in the editor |
| Chord bindings | built | `Profile.chords`; "Combos" list on the Buttons tab |
| Launch app / file / URL | built | `Action::Launch`; opened through Explorer so it runs as the user, not as admin |
| Anti-deadzone | built | `StickConfig.anti_deadzone`; slider per stick in the Library |
| Edge Fn buttons and paddles | built, hw | `fn_l`, `fn_r`, `paddle_l`, `paddle_r` (bit layout from SDL; unverified on a real Edge). Xbox Elite paddles are not reachable: DXO doesn't drive XInput pads |
| Stick response presets | built | Precision / Aggressive / Gentle in the curve box |
| Keyboard/mouse -> virtual controller | **not built** | Needs system-wide keyboard and mouse hooks plus a pad slot with no physical device. Own milestone: a bug in a low-level hook can freeze input for the whole PC, so it must be developed where it can be run |

## 3 · Motion & aim

| Item | Status | Where |
|---|---|---|
| Gyro ratcheting | built | `GyroConfig.ratchet` |
| Flick stick | built | `flick.rs`; needs "pixels per turn" set per game (Motion tab > Test spin) |
| IMU smoothing | built | `gyro.rs`: adaptive Kalman filter on turn rate + gravity tracking for "player space" yaw. Player-space axis signs are unverified on hardware |
| Bezier stick -> mouse curve | built | `Curve::Bezier`, `MouseConfig.stick_curve`; curve editor on the Mouse tab |
| Smarter auto-calibration | built | per-controller gyro bias saved in `controllers.toml`, loaded at connect, refined while the pad rests |

## 4 · Game awareness & automation

| Item | Status | Where |
|---|---|---|
| Fullscreen detect | built | `foreground::fullscreen_app`; touchpad/stick-as-mouse stand down (gyro aim does not) |
| Profile import / export / backup | built | Library > "..." menu; incoming profiles are checked for launch actions |
| Community profiles page | built | **Community** in the sidebar: games as cards with every shared setup, search, filters (kind of game, controller, games on this PC), Get, and "use it for this game?" when another profile has the game. Hub = a public GitHub repo; starter in `dxo-profiles/` (6 profiles, checked by `cargo test`). See `docs/community-hub.md` |
| Steam Input coexistence | built | detection + advice on the Games page. DXO cannot change Steam's settings; the settings-file keys it reads are a best guess and fall back to "unknown" |
| Better auto game-detect | built | GOG added to Steam + Epic; "+ New profile for this game" in the Games list |

## 5 · Output & emulation

| Item | Status | Where |
|---|---|---|
| Virtual DualSense | **groundwork only** | `virtual_ds.rs` builds the input report and parses a game's output report (tested). The device needs a signed HID driver: `docs/virtual-dualsense.md` |
| Adaptive-trigger preset library | built, hw | `TriggerEffect` gained vibration / bow / galloping / machine; 11 presets on the Triggers tab, saved per profile. Encodings follow published notes and are **untested on a DualSense** |
| Audio -> haptics | part built | Bass -> rumble motors on any pad: built, opt-in at build time (`scripts\build.ps1 -AudioHaptics`), **never run on Windows**. DualSense voice-coil haptics: not built (needs a DualSense) |
| Rumble shaping | built | `RumbleShape`: low-rumble boost, ceiling, response |

## 6 · Feedback: light, battery, haptics

| Item | Status | Where |
|---|---|---|
| Lightbar as battery gauge | built | light mode "Battery level" (`light.rs`) |
| Low-battery toast | built | tray notification at 20 % and 10 %; Settings toggle |
| Lightbar effects | built | breathing, pulse, rainbow, flash-on-rumble, per profile |

## 7 · On-screen & overlays

Restructured 12 Nov 2026 and built 13 Nov. The engine parts are unit-tested;
the app built and ran on 13 Nov. Each item still needs its own click-through.

| Item | Status | Where |
|---|---|---|
| Overlay foundation | built | `ui/DXO.UI/Overlay/OverlayWindow.cs`: always-on-top window that never takes focus. `OnScreenKeyboard.cs` shows and hides the overlays as the engine opens them |
| On-screen keyboard | built | Engine: `osk.rs`. The worker in `engine.rs` types with it and shows the game an untouched pad meanwhile. Window: `Overlay/KeyboardOverlay.cs`. PS + Options opens it; Cross on a text box inside DXO opens it; keys can be clicked or tapped. Settings: on/off and size |
| Windows gamepad keyboard (opt-in) | built, untested | Settings (Windows 11 only). PS + Options asks Windows for its own keyboard when the pad is emulated as Xbox 360 (`Overlay/TouchKeyboard.cs`, the keyboard's COM toggle); falls back to DXO's keyboard if Windows refuses. Whether Windows shows its gamepad layout is up to Windows |
| Radial quick-menu | built | Engine: `radial.rs`. Window: `Overlay/RadialOverlay.cs`. PS + R3 by default. Items are editable on the Chords page |
| Chords page (user-mappable) | built | Engine: `chords.rs` is now a table (`SysAction`, `SysBinding`). Page: `Views/ChordsPage`. Saved in settings.json |
| Game launcher page | built, restyled in 7.1 | `Views/LibraryPage` + `Services/GameLibrary.cs` (library.json, art folder): covers, search, genre filter, favourites, add/edit by hand (name, genre, description, program, cover picture), Play. The old profile-assignment page is now "Game profiles", reached from the library. Visual design to be redone against reference screenshots |
| Touchpad multi-touch gestures | built | `pointer.rs`: two-finger scroll (both axes) and pinch to zoom (Ctrl + wheel); switches on the Mouse tab. No three-finger gestures: Sony touchpads report two fingers at most |
| In-game overlay (Game Bar widget) | dropped from this milestone | |

Keyboard and quick-menu rules:

* DXO's own keyboard is the default on every Windows version and in every
  emulation mode. It types into the app in front, with DXO in the tray.
* While the keyboard or quick menu is open, the controller using it is
  neutral to the game (emulated pads only: DXO stops forwarding to the
  virtual pad).
* Native mode (no virtual pad): they work over applications only. With a
  game in front they do not open, and they close if a game comes to the
  front. "A game" = full screen, or has a profile or a controller rule, or is
  in the game library. A windowed game that is none of those is not
  recognised until it is added to the library or given a profile.
* Overlays cannot draw over exclusive-fullscreen games, and Windows blocks
  typed input on the lock screen and UAC prompts. Some anti-cheat games
  ignore it. DXO documents these and does not work around them.

Not in this milestone: Steam cover art fetched automatically, launching
through Steam's own launcher link, touchpad swipe gestures, mouse clicks on
quick-menu items.

## 7.1 · Games page restyle

Agreed 13 Nov 2026, built 15 Nov. **Not built or run on Windows yet.**
Page: `Views/LibraryPage` (row, backdrop, details screen). Online lookups:
`Services/GameMetadata.cs` (Steam store, no key; RAWG and SteamGridDB with the
player's own keys in Settings; pictures kept in `%LOCALAPPDATA%\DXO\art`).
Menu lock: `MainWindow.xaml` (`LeftCompact`, no toggle). The highlighted tile
zooms in place rather than resizing, so the row never shifts.

Second pass (15 Nov, after the first run): one click / Cross opens a game's
details; the details screen follows the console game-card reference
(blurred backdrop, cover + Last played / Time played / Release date, wide
picture with trailer, description, PLAY / achievements / launcher / GAME
DETAILS, button hints). The row slides and the backdrop cross-fades.
Time played is counted by DXO from now on (`Services/PlayTime.cs`); size on
disk is measured from the install folder. Missing descriptions, genres and
release dates are filled in online like pictures. Trailer playback is
**untested**: Steam may serve it as a stream the player can't open.

* Xbox Series X home-screen look: full-screen backdrop of the highlighted
  game, tiles pushed to the bottom.
* One horizontally scrolling row of square tiles, most recently played
  first, then alphabetical.
* Violet outline on the highlighted tile, with its name shown on it.
* Round icons at the very top: "+", rescan, search and genre filter.
* No extra rows, no button hints, no top bar, no side panel.
* Details become a separate screen, opened with Options (Menu on Xbox pads).
* Navigation panel locked collapsed across the whole app.
* Adding a game suggests matches as you type and pulls details and artwork
  online; manual entry still works. Missing backdrops are sourced online,
  else the cover is shown enlarged and dimmed.

## 7.2 · Games page: reviewer requests

From Sean's review of 0.3.0 (15 Nov 2026). All are settings under
Settings > Games page. Built 15 Nov, **not built or run on Windows yet**.

| Item | Status | Where |
|---|---|---|
| Tile size | built | `AppSettings.GameTileScale` (75-150 %); tiles bind `LibraryItem.TileWidth/TileHeight` |
| Living backdrop (slow zoom and drift) | built | `LibraryPage.StartLivingBackdrop` animates `BackdropMotion`; on by default |
| Animated backgrounds (GIF or video, per game) | built | picked in Game details > Edit; GIFs play in the backdrop images, videos (MP4, WebM, MOV) in `BackdropVideo`, muted and looped, only for the highlighted game |
| Three backgrounds per game, slideshow or pick one | built | `GameLibrary.AddBackdrop` keeps up to 3 (duplicates skipped by content); sources: Steam hero + screenshots, SteamGridDB heroes. "Pick one" shows thumbnails on the details screen; "Slideshow" fades every 12 s |
| Scan a games folder (e.g. C:\Games) | **not built** | Add every game in a chosen folder, guessing the title from the folder name and the right .exe, and asking when unsure; then fetch art and suggest a community profile |

## 8 and beyond

To be restructured. Carried over from the PDF: accessibility (8), reliability
& diagnostics (9), engine as a service (10), UI overhaul (11), distribution &
trust (12). To be placed: keyboard/mouse -> virtual controller, virtual
DualSense driver, DualSense voice-coil haptics, DS4 Bluetooth speaker audio.

## Not doing

Aim-assist, anti-recoil, anything built to defeat games or evade anti-cheat.
Lock-screen unlock stays its own signed-driver milestone
(`docs/route2-lockscreen.md`).
