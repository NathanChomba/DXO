# DXO roadmap

Every milestone ends with something you can use. Status key:
**done** = implemented and unit-tested; **verify** = written but needs
hardware testing on Windows; **todo** = stub or notes only.

## M1: Read the controller (USB)
Goal: `dxo monitor` shows live sticks, buttons, gyro and battery.

| File | Status |
|---|---|
| `device/mod.rs`: enumerate, open, poll loop | verify |
| `device/dualsense.rs`: DualSense/Edge USB + BT report parsing | done (tests), verify |
| `device/ds4.rs`: DualShock 4 USB + BT (full 0x11 and reduced 0x01) parsing | done (tests), verify |
| `state.rs`: normalised `PadState` | done |
| CLI: `list`, `raw`, `monitor` | verify |

Exit check: every button lights up in `monitor`; sticks reach ±1.00; battery looks right.

## M2: Talk back to the controller
Goal: change lightbar, rumble, and adaptive triggers from the CLI.

| File | Status |
|---|---|
| `output.rs`: DualSense USB report 0x02, DS4 USB 0x05 and BT 0x11 (+CRC) | done (tests), verify |
| `output.rs`: trigger effects (off / feedback / weapon) | verify |
| `output.rs`: DualSense BT report 0x31 + CRC | written, test on BT in M6 |
| CLI: `lightbar`, `rumble`, `trigger` | verify |

Exit check: `dxo lightbar 255 0 0` turns it red; `trigger weapon` clicks.
If the lightbar ignores you, set `valid_flag2` / `lightbar_setup` once at
connect (see comments in `output.rs`).

## M3: Play games with it
Goal: games see an Xbox 360 pad and never see the real one.

Install first: ViGEmBus and HidHide (both by Nefarius).

| File | Status |
|---|---|
| `virtual_pad.rs`: `VirtualPad` trait, ViGEm Xbox 360 backend | verify |
| `hidhide.rs`: hide/unhide via HidHideCLI | verify (instance id may need the parent device) |
| `mapping.rs`: deadzones, curves, remaps | done (tests) |
| `engine.rs`: engine thread, reconnect, output keep-alive | verify |
| `timing.rs`: high-resolution waitable timer | verify |
| CLI: `run` | verify |
| todo: forward game rumble (ViGEm notifications) | todo |

Exit check: run `dxo run`, open a game or a gamepad tester, only one
controller appears, rumble works (after the todo).

## M4: Desktop app
Goal: WinUI 3 window with a live dashboard.

| File | Status |
|---|---|
| `ffi.rs`: C ABI, `#[repr(C)]` structs, panic guards | done (layout test) |
| `ui/.../Native/DxoNative.cs`: P/Invoke wrapper | verify |
| `ui/.../Views/DashboardPage`: sticks, triggers, buttons, lightbar picker, trigger test | verify |
| Settings page: driver status, "unhide all" safety button | todo |
| Tray icon, start with Windows | todo |

Exit check: `scripts\build.ps1 -Run` opens the window and the dots move.

## M5: Profiles and motion
| File | Status |
|---|---|
| `profile.rs`: TOML profiles, load folder | done (tests) |
| `foreground.rs` + engine: auto-switch by game exe | verify |
| `gyro.rs`: CemuHook (DSU) server on UDP 26760 | verify (axis signs in Cemu's motion test) |
| `gyro.rs`: per-device calibration from feature reports | todo |
| Gyro to mouse / right stick | todo |
| Profiles page editor (Tomlyn for TOML in C#) | todo |
| Touchpad as mouse | todo |

## M6: Stretch goals
| Area | Status |
|---|---|
| `device/generic.rs`: HID-descriptor parser for generic pads | done (tests), needs real pads; button order per device |
| Bluetooth: DualSense output | written, untested |
| Bluetooth idle disconnect | todo |
| `haptics.rs`: audio to rumble (WASAPI loopback + envelope) | envelope done, capture todo |
| DualSense voice-coil haptics over USB (4-channel endpoint) | todo |
| Stop Windows switching default audio to the pad | todo |
| BT audio / mic (SBC over HID) | research only |
| Virtual DualSense backend (HIDMaestro or similar) | research; check the project is real and maintained first |
| UDP telemetry to adaptive triggers (`TriggerEffect::Raw`) | todo |

## Not doing (yet)
- Kernel tricks to raise the timer resolution globally: the waitable timer covers it.
- Claiming 1000 Hz on Bluetooth: the pad's own report rate is the ceiling.
