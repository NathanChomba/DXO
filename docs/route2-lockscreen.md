# Route 2 — Controller Unlock of the Windows Lock Screen (Milestone L)

**Status:** parked / future milestone. Not the next build.
**Goal:** let a connected controller type the Windows account PIN at the real
lock screen (LogonUI), PS5-style — genuine credential entry, not a convenience
lock.

## Core principle

The Windows secure desktop blocks `SendInput` and all user-mode synthetic input
**by design**. The only honest way to type at LogonUI is to **be a real HID
keyboard**. Windows accepts a genuine keyboard at the lock screen the same way
it accepts a USB one.

- Do **not** write secure-desktop- or LogonUI-specific injection code. If the
  design ever needs to "target" LogonUI, it is wrong.
- The feature is "DXO presents a real keyboard"; the lock screen working is a
  side effect of being a keyboard, not something engineered around.

## Non-negotiable security gate (hard requirement)

The virtual keyboard emits a keystroke **only** in direct response to a live
physical button press on the connected controller.

- No "type this string" / arbitrary-text API.
- No timer-, script-, or network-driven emission.
- The driver accepts reports only from DXO's own authenticated service (verify
  the caller's identity / signature).
- Mapping (physical press → keycode) stays as close to the driver as possible;
  no fat user-mode "type anything" surface.

**Rationale:** the instant arbitrary software can push keystrokes into this
endpoint, it is a pre-authentication credential-injection backdoor. Physical
presence is the security model — worst case is "attacker has the PC *and* the
controller," which is no worse than a plugged-in USB keyboard.

## Components

1. **Virtual HID keyboard driver** — UMDF2, derived from Microsoft's `vhidmini`
   sample, exposing a standard boot-keyboard report descriptor (8-byte reports:
   modifier byte + up to 6 keycodes). Windows sees a real keyboard.
2. **Engine mapping** — the existing Rust engine reads live controller HID input
   and maps buttons → HID usage codes, emitted through the driver, under the
   security gate above.
3. **Lock-state awareness** — `WTSRegisterSessionNotification`; on
   `WTS_SESSION_LOCK` activate the PIN map, on `WTS_SESSION_UNLOCK` deactivate.
   This is session-state awareness only, never injection.
4. **PIN keymap** — PS5-style grid (D-pad / face / shoulder buttons → digits
   0–9). Trivial; purely a key map.

## Build sequence

- **L1 — Virtual keyboard driver.** `vhidmini`-derived UMDF2, test-signed,
  proven in a **VM**: types into Notepad, then the lock screen. No DXO wiring
  yet.
- **L2 — Engine ↔ driver channel.** Rust engine → driver, security gate
  enforced; driver authenticates the caller.
- **L3 — Lock awareness + keymap.** Session notifications activate the PIN map
  only when locked; wire the grid.
- **L4 — Signing.** Test-signing (dev only; needs test mode / weakened signature
  enforcement on the dev machine). Distribution = EV cert + attestation signing
  or WHQL via Partner Center (paid, slow). Decide personal-only vs shippable.

## Caveats

- Kernel/driver tier: higher blast radius than anything in DXO today. A driver
  bug can BSOD or break boot. **All early work in a VM.**
- Distribution signing is a real recurring cost and gate.
- Test-signing on your own machine weakens its signature-enforcement posture —
  note for personal use.
- Sequence this **after** the smaller user-space wins (fullscreen-detect to
  suspend mouse emulation, gyro filter, Bézier stick curve, radial menu), which
  ship on the current stack with no driver work.

## Explicitly out of scope

- Any input designed to be indistinguishable from hardware in order to evade
  anti-cheat or defeat a security boundary.
- Routing gameplay input through the keyboard endpoint. The gamepad stays
  ViGEmBus; the keyboard endpoint is desktop/lock-screen only and does nothing a
  keyboard can't.
- Any software-writable or remote-triggerable path into the keyboard endpoint.
