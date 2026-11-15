#!/usr/bin/env python3
"""Check index.json and every profile before a pull request can merge.

Run locally from the repository root:  python tools/validate.py
Needs Python 3.11+ (tomllib) and `pip install jsonschema`.
On a pull request it also checks that changed profiles had their version raised.
"""
import json
import os
import pathlib
import subprocess
import sys
import tomllib

import jsonschema

ROOT = pathlib.Path(__file__).resolve().parent.parent
MAX_BYTES = 32 * 1024

# Top-level settings DXO understands (crates/dxo-core/src/profile.rs, `Profile`).
KNOWN = {
    "name", "match_process", "target", "left_stick", "right_stick", "trigger_deadzone", "remap",
    "bindings", "lightbar", "player_leds", "hide_physical", "rumble_passthrough", "rumble_strength",
    "rumble_swap", "controller_audio", "audio_volume", "gyro", "shift_button", "shift_bindings",
    "mouse", "chords", "hold_ms", "double_tap_ms", "trigger_soft_point", "trigger_full_point",
    "trigger_left", "trigger_right", "light", "rumble_shape", "audio_haptics", "flick",
}

errors: list[str] = []


def fail(msg: str) -> None:
    errors.append(msg)


def has_launch(value) -> bool:
    if isinstance(value, dict):
        if value.get("type") == "launch":
            return True
        return any(has_launch(v) for v in value.values())
    if isinstance(value, list):
        return any(has_launch(v) for v in value)
    return False


def git(*args: str) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=True).stdout


def main() -> int:
    try:
        index = json.loads((ROOT / "index.json").read_text(encoding="utf-8"))
    except Exception as e:
        print(f"index.json: not valid JSON: {e}")
        return 1

    schema = json.loads((ROOT / "schema" / "index.schema.json").read_text(encoding="utf-8"))
    for err in jsonschema.Draft202012Validator(schema).iter_errors(index):
        where = "/".join(str(p) for p in err.absolute_path) or "index.json"
        fail(f"{where}: {err.message}")
    if errors:
        return report()

    entries = index["profiles"]
    seen_ids, seen_urls, seen_names = set(), set(), set()
    for e in entries:
        label = e["id"]
        for key, seen in (("id", seen_ids), ("url", seen_urls), ("name", seen_names)):
            if e[key].lower() in seen:
                fail(f"{label}: another entry already uses this {key} ({e[key]})")
            seen.add(e[key].lower())

        path = ROOT / e["url"]
        if not path.is_file():
            fail(f"{label}: {e['url']} does not exist")
            continue
        if path.stat().st_size > MAX_BYTES:
            fail(f"{label}: {e['url']} is over 32 KB")
        try:
            profile = tomllib.loads(path.read_text(encoding="utf-8"))
        except Exception as ex:
            fail(f"{label}: {e['url']} is not valid TOML: {ex}")
            continue
        unknown = sorted(set(profile) - KNOWN)
        if unknown:
            fail(f"{label}: settings DXO doesn't know: {', '.join(unknown)}")
        if profile.get("name") != e["name"]:
            fail(f"{label}: name in the file ({profile.get('name')!r}) differs from index.json ({e['name']!r})")
        if not profile.get("match_process"):
            fail(f"{label}: match_process is empty; list the game's .exe so the profile switches on by itself")
        if has_launch(profile):
            fail(f"{label}: contains an \"open a program\" (launch) action; remove it")

    # Every .toml must be listed.
    listed = {(ROOT / e["url"]).resolve() for e in entries}
    for f in sorted((ROOT / "profiles").rglob("*.toml")):
        if f.resolve() not in listed:
            fail(f"{f.relative_to(ROOT).as_posix()}: not listed in index.json")

    check_versions(entries)
    return report()


def check_versions(entries) -> None:
    """On a pull request: a changed profile needs a higher version and a new date."""
    base = os.environ.get("GITHUB_BASE_REF")
    if not base:
        return
    try:
        changed = set(git("diff", "--name-only", f"origin/{base}...HEAD").split())
        old = {p["url"]: p for p in json.loads(git("show", f"origin/{base}:index.json")).get("profiles", [])}
    except Exception as ex:
        print(f"note: skipped the version check ({ex})")
        return
    for e in entries:
        before = old.get(e["url"])
        if e["url"] in changed and before is not None:
            if e["version"] <= before.get("version", 0):
                fail(f"{e['id']}: the file changed, so raise version above {before.get('version')}")
            if e["updated"] == before.get("updated"):
                fail(f"{e['id']}: the file changed, so set updated to today's date")


def report() -> int:
    if errors:
        print(f"{len(errors)} problem(s):")
        for msg in errors:
            print(f"  - {msg}")
        return 1
    print("All profiles look good.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
