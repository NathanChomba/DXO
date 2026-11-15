# Virtual DualSense: plan

Roadmap section 5. Status: **groundwork built, device not built.**

## The goal

A game that supports the DualSense natively (adaptive triggers, haptics,
touchpad, gyro) should see a real DualSense when DXO is running, and DXO
should pass what the game sends on to the physical pad. Today DXO can present
an Xbox 360 pad or a DualShock 4 through ViGEmBus; ViGEm has no DualSense
target, so games fall back to Xbox behaviour and their trigger effects never
arrive.

## What is built (crates/dxo-core/src/virtual_ds.rs)

The half that doesn't depend on any driver, unit-tested against DXO's own
DualSense parser and output builder so both directions agree:

| Piece | What it does |
|---|---|
| `build_input_report(&PadState, counter)` | The 64-byte USB input report (id 0x01) a real DualSense would send for a given state: sticks, triggers, buttons, hat, gyro, accelerometer, both touch points, battery. |
| `parse_output_report(&[u8])` | Reads a game's output report (id 0x02) into rumble, both trigger effects (kept as the raw 11-byte block), light bar, player LEDs and mic LED, honouring the report's "valid" flags so unset fields are left alone. |
| `GameOutput::apply_to(&mut OutputState)` | Lays the game's wishes over what DXO would send; the result goes to the real pad through the existing `output::build_report`. |

Tests prove: a built input report reads back identically through
`device::dualsense::parse_input`; an output report built by DXO and parsed as
if a game had sent it forwards to the pad byte-for-byte.

## What is not built: the device itself

Windows needs a driver to make a HID device appear. Options, in order of
preference:

1. **A maintained open-source virtual-HID driver that DXO can drive from user
   mode.** The first thing to do is survey what exists *today* and is signed
   and maintained (ViGEmBus is no longer being developed; check what
   replaced it and whether any successor exposes arbitrary HID report
   descriptors). If one does, DXO only needs: the DualSense report descriptor,
   the feature reports below, and a loop that calls the two functions above.
2. **Our own UMDF2 virtual HID driver** (Microsoft's Virtual HID Framework,
   `Vhf`, is the supported way to create a HID device from a driver). It must
   be signed: a test-signed driver only loads with test mode on, and a
   release-signed one needs an EV certificate and Microsoft attestation
   signing. This is the same cost as the lock-screen driver in
   `docs/route2-lockscreen.md`, so the two should share one driver project and
   one certificate.

Either way the device has to answer the requests a game (or Steam, or SDL)
makes when it opens a DualSense:

| Request | Needed content |
|---|---|
| HID report descriptor | The DualSense USB descriptor (input 0x01, output 0x02, features 0x05, 0x08, 0x09, 0x20, 0x22 ...). |
| Feature 0x05 | IMU calibration block. A neutral calibration is fine; games scale gyro with it. |
| Feature 0x09 | Pairing info: the pad's MAC address. Use the physical pad's. |
| Feature 0x20 | Firmware / hardware version. Report a recent real version: some games refuse very old firmware. |
| VID/PID | 054C:0CE6 (DualSense). |

## Order of work when this milestone starts

1. Survey current virtual-HID options (an afternoon, no code).
2. Capture the descriptor and feature reports from a real DualSense over USB
   (`dxo raw` plus a descriptor dump). **Needs a DualSense on the desk.**
3. Bring up the device with a static input report; confirm Windows' game
   controller panel and one DualSense-aware game list it.
4. Wire `build_input_report` in, then `parse_output_report` -> real pad.
5. Add `VirtualTarget::DualSense` to profiles, the controller card and the
   Virtual Device page.
6. Audio haptics need the virtual pad to expose the 4-channel audio endpoint
   too; treat that as a second phase.

## Fair play

A virtual DualSense is a standard HID device, visible to anti-cheat like any
other. Nothing here hides DXO or imitates hardware to defeat detection.
