# Changelog

All notable changes to DXO. Versions follow [Semantic Versioning](https://semver.org);
while DXO is below 1.0, any release may change settings or profiles.

## [Unreleased]

### Added
- Games page settings (Settings > Games page), from reviewer feedback:
  - **Tile size:** make the game covers bigger or smaller.
  - **Living backdrop:** the artwork slowly zooms and drifts so it feels alive.
  - **Animated backgrounds:** give a game a GIF or a short looping video
    (MP4, WebM, MOV) as its background, under Game details > Edit.
  - **Several backgrounds:** DXO keeps up to three backgrounds per game. Choose
    your favourite on the game's details screen, or let them play as a slideshow.

## [0.3.1] - unreleased

A fix-up release for 0.3: steadier input timing and a safety net for crashes.

### Fixed
- **Input path no longer allocates.** The controller loop used to copy the
  whole profile and build temporary lists for every input report (up to
  1,000 a second). It now shares the profile, resolves button names without
  allocating, and reuses its buffers, so input timing no longer depends on
  the memory allocator.
- **Lightbar, rumble and triggers no longer hold up input.** Output reports
  are written on their own thread through a second handle to the controller.
  A slow Bluetooth write used to delay the next input report; now only the
  newest output report is kept and written when the controller is ready.
- **A crash no longer leaves your controller hidden from games.** DXO now
  keeps a list of the controllers it hides (`%LOCALAPPDATA%\DXO\hidden.txt`).
  If DXO crashes or is killed, the next start unhides them first, and
  uninstalling DXO unhides them too.

## [0.3.0] - 2026-11-15

The biggest release so far: everything on the road map from milestone 1 to 7.1.
DXO now goes past mapping buttons, with an on-screen keyboard and a quick menu
that work in any app, chords you can remap, touchpad gestures, and a
console-style games library.

### Highlights

- **On-screen keyboard.** Hold PS and press Options to type into any app with
  the controller, with DXO in the tray. Keys can also be clicked or tapped.
- **Quick menu.** PS + R3 opens a ring of actions in the middle of the screen
  (media, volume, screenshot, keyboard, or your own).
- **Chords page.** Choose what every PS + button combination does, including
  key combinations and opening apps, files or links.
- **Games library.** Your games as a console-style page: the highlighted
  game's artwork fills the screen, and each game has a details screen with
  its trailer, time played and a Play button. Pictures and descriptions are
  looked up online.
- **Touchpad gestures.** Two fingers to scroll in any direction, pinch to zoom.

### Added

#### On-screen and overlays (milestone 7)
- Overlay windows that float over other apps without taking focus from them.
- On-screen keyboard, controller-first:
  - D-pad or left stick moves the highlight and Cross types.
  - Square is Backspace, Triangle is Space, Options is Enter and Circle closes.
  - L3 is Shift (twice for caps lock), L2 switches to symbols, L1 / R1 move the cursor.
- Cross on a text box inside DXO opens the keyboard. Settings has an on/off
  switch and a size slider.
- Optional: use Windows 11's own gamepad keyboard instead, for a controller
  emulated as an Xbox 360 pad (Settings, Windows 11 only, experimental).
- Quick menu: point with a stick and let go, or press Cross. Up to 12 items,
  edited on the Chords page.
- Chords page, where every chord can be reassigned. The defaults are unchanged,
  plus PS + Options for the keyboard and PS + R3 for the quick menu.
- While the keyboard or quick menu is open, the controller using it is hidden
  from the game.

#### Games library (milestones 7 and 7.1)
- One horizontally scrolling row of game covers, most recently played first.
- The highlighted game's backdrop fills the page and cross-fades as you move.
  The row slides smoothly and the highlighted cover zooms slightly.
- The game's full name and a status line ("Played 1h 46m · last played
  2 days ago", "Not installed") sit under the highlighted cover, Big Picture
  style.
- Round buttons at the top to search, filter by genre or favourites, rescan,
  and add a game.
- Details screen, opened with one click, Cross, or Options:
  - Cover, last played, time played and release date.
  - The wide picture, with the trailer playing inline.
  - The description.
  - Buttons for Play, Steam achievements, open in Steam / Epic, and
    Game details (edit, favourite, game profiles, install folder, remove).
- Finds Steam, Epic and GOG games. Add any other game by hand with a name,
  genre, description, program, cover and background.
- Typing a name while adding a game suggests matches. Picking one fills in
  the details and downloads the pictures.
- Missing pictures and descriptions are filled in online, once per game.
- Online sources:
  - Steam's store, which needs no key.
  - SteamGridDB and RAWG, using free personal keys entered in Settings.
- One Settings switch turns all online lookups off. Pictures are stored on
  your PC.
- Time played is counted while a game runs, however it was started.
- Size on disk is shown in the details.
- Covers are never cropped. Pictures of other shapes are shown whole.

#### Touchpad (milestone 7)
- Two-finger scrolling, up / down and sideways. The page follows your fingers.
- Pinch to zoom in apps that zoom with Ctrl + mouse wheel.
- Switches for both on the Mouse tab.

#### Feedback: light, battery, haptics (milestone 6)
- Lightbar as a battery gauge.
- Low-battery notification at 20 % and 10 %.
- Lightbar effects per profile: breathing, pulse, rainbow and flash on rumble.

#### Output and emulation (milestone 5)
- Adaptive-trigger preset library: 11 presets (bow, machine gun, galloping
  and more), saved per profile.
- Rumble shaping: low-rumble boost, ceiling and response.
- Audio to rumble: game bass drives the rumble motors on any controller
  (opt-in at build time).

#### Game awareness and automation (milestone 4)
- Mouse emulation stands down when a game goes full screen.
- Profile import, export and backup.
- Community page: browse and download profiles others shared, per game,
  filtered by kind of game and controller. The hub is a public GitHub repo.
- Steam Input detection, with advice to avoid double input.
- GOG games are detected alongside Steam and Epic, with
  "+ New profile for this game".

#### Motion and aim (milestone 3)
- Gyro ratcheting (hold a button to lift the "mouse").
- Flick stick.
- Smoother, steadier gyro (sensor filtering plus "player space" turning).
- Bezier curve for stick-to-mouse.
- Gyro calibration remembered per controller.

#### Mapping depth (milestone 2)
- Dual-stage triggers: soft pull and full pull do different things.
- Tap, hold and double-tap actions on one button.
- Combo bindings (two buttons together).
- A button can launch an app, file or web address.
- Anti-deadzone per stick.
- Stick response presets: Precision, Aggressive, Gentle.
- DualSense Edge function buttons and back paddles.

#### Controller-first polish (milestone 1)
- Number boxes adjust with the D-pad.
- The controller strip can be navigated with the pad.
- A clear starting highlight, and each page remembers where you were.

### Changed
- Two downloads per release: a small one that needs the .NET 10 Desktop
  Runtime installed once, and a "-with-dotnet" one that includes it.
- The navigation menu is locked to its icons-only strip on every page.
  Options on the controller now opens a game's details on the Games page.
- The old Games page (which profile each game uses) is now **Game profiles**,
  reached from the library.
- System chords are a table the Chords page edits, instead of fixed combinations.
- Version 0.3.0. The engine's interface to the app moved to ABI 13; an older
  `dxo_core.dll` won't load with this app.

### Fixed
- The app crashed when going back from a game's details after playing its trailer.
- A backdrop picture could cover the whole Games page and block clicks.

### Known limitations
- Unsigned build: Windows shows "unknown publisher" when DXO asks for
  administrator rights.
- Overlays (keyboard, quick menu) cannot draw over exclusive-fullscreen games,
  and Windows blocks typed input on the lock screen and UAC prompts. Some
  anti-cheat games ignore it. DXO documents this and does not work around it.
- For a controller in Native mode (no virtual pad), the keyboard and quick
  menu work over apps only, never over a game.
- Time played starts counting from this version; earlier hours aren't known.
- Trailers depend on how Steam serves them and may not play for every game.
- **Needs testing on hardware:** DualSense Edge paddles, adaptive-trigger
  presets, gyro "player space" direction, and audio-to-rumble.
- Not in this release: virtual DualSense output (needs a signed driver),
  keyboard/mouse to virtual controller, DualSense voice-coil haptics.

## [0.2.0] - 2026-11-04

First review build.

- Devices: DualSense, DualSense Edge, DualShock 4, Switch Pro and generic
  pads; up to 4 at once, each remembered across reconnects.
- Virtual Xbox 360 or DualShock 4 controller (ViGEm), real controller hidden
  from games (HidHide).
- Remaps, keyboard and mouse output, turbo, toggle, macros, shift layer.
- Gyro to mouse, CemuHook motion server for emulators, touchpad and stick as mouse.
- Lightbar, rumble, adaptive triggers, controller speaker audio.
- Per-game profiles with automatic switching.
- The whole app works with a controller.
- System chords (PS + button).
- Tray, start with Windows, installer, diagnostics.

[0.3.1]: https://github.com/NathanChomba/DXO/releases/tag/v0.3.1
[0.3.0]: https://github.com/NathanChomba/DXO/releases/tag/v0.3.0
[0.2.0]: https://github.com/NathanChomba/DXO/releases/tag/v0.2.0
