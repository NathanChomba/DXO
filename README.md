# DXO

Open-source controller mapper for Windows: DualShock 4, DualSense, DualSense
Edge, and generic USB pads. Rust engine, WinUI 3 front end.

## Layout
```
dxo/
├── Cargo.toml                 Rust workspace
├── crates/
│   ├── dxo-core/         engine (rlib + dxo_core.dll)
│   │   └── src/
│   │       ├── device/        M1  dualsense.rs, ds4.rs, generic.rs (M6)
│   │       ├── state.rs       M1  normalised PadState
│   │       ├── output.rs      M2  lightbar, rumble, adaptive triggers
│   │       ├── virtual_pad.rs M3  ViGEm Xbox 360 pad
│   │       ├── hidhide.rs     M3  hide the physical pad
│   │       ├── mapping.rs     M3  deadzones, curves, remaps
│   │       ├── timing.rs      M3  high-resolution timer
│   │       ├── engine.rs          the engine thread that ties it together
│   │       ├── ffi.rs         M4  C ABI for the UI
│   │       ├── profile.rs     M5  TOML profiles
│   │       ├── foreground.rs  M5  per-game profile switching
│   │       ├── gyro.rs        M5  CemuHook / DSU motion server
│   │       └── haptics.rs     M6  audio haptics (notes + envelope follower)
│   └── dxo-cli/          test bench: list / raw / monitor / lightbar / run ...
├── ui/DXO.UI/            M4  WinUI 3 app (unpackaged)
│   ├── Themes/DxoTheme.xaml   colours and styles (change DxoAccentColor to re-skin)
│   ├── ViewModels/            DeviceViewModel: one shared, polled controller state
│   └── Views/                 Home, My Controller, Virtual Device, Triggers, Lighting, ...
├── profiles/                  default.toml, example-game.toml
├── scripts/build.ps1          build everything
├── ROADMAP.md                 milestone checklist with status per file
└── DESIGN.md                  UI layout, page map and visual tokens
```

## Prerequisites (Windows)
- Rust (stable, MSVC toolchain): https://rustup.rs
- .NET 10 SDK and Visual Studio 2022 with the "Windows App SDK C#" workload (M4)
- ViGEmBus and HidHide (M3), both from Nefarius on GitHub

## Getting started
```powershell
cargo test                                   # 22 unit tests, no hardware needed
cargo run -p dxo-cli -- list            # M1
cargo run -p dxo-cli -- monitor         # M1
cargo run -p dxo-cli -- lightbar 255 0 0   # M2
cargo run -p dxo-cli -- trigger weapon     # M2 (DualSense)
cargo run -p dxo-cli -- run profiles    # M3+: virtual pad + HidHide
.\scripts\build.ps1 -Run                      # M4: the desktop app
```

## Design rules
- One engine thread owns the HID handle. The UI reads snapshots over FFI and never touches HID.
- Every parser normalises into `PadState`; nothing downstream knows which pad it came from.
- Byte offsets cite their source (Linux `hid-playstation.c`) and each parser has tests with synthetic reports.
- `VirtualPad` is a trait so a virtual-DualSense backend can replace ViGEm later without touching the engine.

## Safety
If the app crashes while HidHide is cloaking your pad, games won't see the
controller. Open the HidHide Configuration Client and untick the device.
(A "Unhide all" button is on the M4 todo list.)
