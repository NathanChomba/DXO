# DXO design notes

Source: 24 reference screenshots of DSX v3.1 / v3.0.5 supplied by Caesar.
DXO borrows the **layout, information architecture and dark-plum mood**.
It does **not** copy DSX's name, logo, icons, hero artwork, controller
renders or exact wording. Everything visual that is "art" is DXO's own.

## 1. Window structure

```
┌──────────────────────────────────────────────────────────────┐
│ ◆ DXO  v0.1                                     ─  ☐  ✕      │  custom title bar
├──────────┬───────────────────────────────────────────────────┤
│ ≡        │ [● pad icon] Wireless Controller   BT · 75% · 250Hz │  device header card
│ Home     ├───────────────────────────────────────────────────┤
│ My pad   │                                                   │
│ Profiles │                 page content                       │
│ ─────    │      (cards / two-pane config / grids)            │
│ Mappings │                                                   │
│ Virtual  │                                                   │
│ Triggers │                                                   │
│ Lighting │                                                   │
│ Haptics  │                                                   │
│ Motion   │                                                   │
│ ─────    │                                                   │
│ Games    │                                                   │
│ Settings │                                                   │
│ About    │                                                   │
└──────────┴───────────────────────────────────────────────────┘
```

- Left nav, always expanded on wide windows, compact (icons) below ~1000 px.
- **Device header card** sits above every page: status dot, controller name,
  active profile, transport icon, battery %. Click = My Controller page.
  When nothing is connected it becomes a warning strip: "No controller
  detected - connect over USB or Bluetooth".
- Content scrolls; the header and nav do not.

## 2. Page map (milestone each page becomes real)

| Nav item | What it shows | Milestone |
|---|---|---|
| Home | Hero card (DXO name + tagline over DXO's own abstract art), quick-action tiles (Controllers, Profiles, Settings, Games, Help), "What's new" expander | M4 |
| My Controller | Device info grid (name, type, VID:PID, connection, battery, polling rate, firmware), supported-features grid with ticks/crosses | M4 |
| Profiles | Search box, create/import/open-folder rows, profile cards with Apply / Edit / Duplicate / Delete | M5 |
| Mappings | **Two-pane**: controller diagram left (click a button to select it, Physical/Virtual toggle), action list right (Button, Keyboard, Mouse, Shortcut, Turbo, Toggle) with breadcrumb `Controller › Actions › …` | M5 |
| Virtual Device | **Two-pane**: emulation mode list left (Native, Xbox 360, DualShock 4, DualSense), mode options right (rumble/trigger/LED passthrough), plus Fix Double Input, key repeat | M3/M4 |
| Triggers | **Two columns**: Left trigger / Right trigger, each a radio list of modes (Off, Feedback, Weapon, Vibration, Slope…) that expands to sliders | M4 (DualSense only) |
| Lighting | Preset bar (dropdown + add/duplicate/delete), tabs Light bar / Indicators, brightness slider, mode list (Static, Multi, Breathing, Strobe, Battery), colour wheel + RGB/hex + swatches, Player LED patterns | M4 |
| Haptics | Rumble on/off, intensity, enhanced rumble, audio-to-rumble; later the audio-to-haptics list with source device, latency, EQ | M6 |
| Motion | Mode tabs (Off, To Mouse, To Stick, CemuHook), deadzone and sensitivity sliders, axis toggles | M5 |
| Games | Grid of detected games with art, per-game profile assignment, launch | M5+ |
| Settings | Start with Windows, tray, driver status (ViGEmBus / HidHide), **Unhide all controllers** safety button, logs | M4 |

## 3. Components (and the WinUI control behind each)

| Pattern in the refs | DXO control |
|---|---|
| Row: icon + title + grey description + control on the right | `SettingsCard` (CommunityToolkit.WinUI.Controls.SettingsControls) |
| Row that expands to sliders | `SettingsExpander` |
| Row with chevron that navigates | `SettingsCard IsClickEnabled="True"` |
| Big rounded panel | `Border Style="{StaticResource DxoCard}"` |
| Segmented icon tabs (deadzone L/R/L2/R2, motion modes) | `SelectorBar` |
| Breadcrumb `Controller › Deadzones` | `BreadcrumbBar` |
| Physical / Virtual pill toggle | `SelectorBar` with two items |
| Feature grid with ticks | `ItemsRepeater` + `UniformGridLayout` |
| Colour wheel + swatches | `ColorPicker` (ring) + `GridView` of swatches |

## 4. Visual tokens (all in `Themes/DxoTheme.xaml`)

Sampled from the references, then given DXO's own accent.

| Token | Value | Use |
|---|---|---|
| `DxoWindowTop` / `DxoWindowBottom` | `#3A2F40` → `#1E1624` | window background gradient |
| `DxoNav` | `#221C29` | sidebar |
| `DxoSurface` | `#2F2838` | cards |
| `DxoSurfaceRaised` | `#3B3246` | rows inside cards, hover |
| `DxoStroke` | `#FFFFFF` @ 8 % | 1 px card borders |
| `DxoAccent` | `#B26BFF` | toggles, sliders, selected nav bar, primary buttons. **One line to change.** |
| `DxoAccentSoft` | accent @ 18 % | selected row fill |
| `DxoSuccess` / `DxoWarning` / `DxoDanger` | `#3DDC84` / `#F2B544` / `#FF5A6E` | connected dot, warnings, delete |
| `DxoTextSecondary` | `#FFFFFF` @ 62 % | descriptions |
| Corner radius | 8 px cards, 6 px rows | |
| Spacing | 4 / 8 / 12 / 16 / 24 | |
| Type | Segoe UI Variable; titles 20 semibold, card titles 14 semibold, descriptions 12 | |

The app is dark-only by default (like the refs). A light theme is a later nicety.

## 5. DXO-specific rules
- Features the connected pad can't do are **shown but disabled**, with a one-line reason
  ("Adaptive triggers need a DualSense"). Caesar's test pad is a DS4, so this matters early.
- Every destructive action (delete profile, unhide all) asks for confirmation.
- Controller diagram is DXO's own flat line-art SVG, not a photo or a copy of Sony art.
- No ads, no account, no telemetry. Say so on the About page.
