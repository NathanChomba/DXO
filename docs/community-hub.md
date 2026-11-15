# Community profile hub

DXO starts on its own hub, github.com/NathanChomba/DXO-Profiles, and contacts
it only while the **Community** page is open (the gear button changes or
disconnects it). It only downloads; nothing is uploaded.

## What a hub is

A public GitHub repository: one `index.json` plus the profile `.toml` files
it lists. It costs nothing, anyone can read it, and people add profiles with
pull requests that the hub's owner reviews. `dxo-profiles/` in this repo is
the complete repository (profiles, checks, templates): upload it as `dxo-profiles` and
point DXO at the raw address of its index:

    https://raw.githubusercontent.com/<owner>/<repo>/main/index.json

Any https host serving the same files works too.

## index.json

```json
{
  "profiles": [
    {
      "name": "CoD: Gyro aim",
      "game": "Call of Duty",
      "category": "Shooter",
      "author": "Caesar",
      "controller": "DualSense",
      "description": "Hold L2 to aim with the gyro.",
      "tags": ["gyro", "adaptive triggers"],
      "url": "profiles/cod-gyro-aim.toml"
    }
  ]
}
```

| Field | Required | Meaning |
|---|---|---|
| `name` | yes | The setup's name. Should match `name` inside the .toml. |
| `url` | yes | The `.toml`, absolute (`https://...`) or relative to `index.json`. Must be https. |
| `game` | recommended | Groups setups on the Community page; several per game is normal. |
| `category` | recommended | One of: Shooter, Racing, Action & adventure, Sports, Fighting, Platformer, Strategy, Flight & simulation, Emulation, Desktop, Other. Anything else shows as Other. |
| `controller` | no | What it was made for ("DualSense", "DualSense Edge", "DualShock 4"...). Empty or "Any" = any pad. |
| `author`, `description` | no | Shown under the name. |
| `tags` | no | Up to 8 short words; searchable. |

Entries without a `name` or with a non-https `url` are ignored.

## The Community page

* games are cards, each listing every shared setup for that game;
* search matches game, setup name, author, controller, tags and description;
* filters: kind of game, controller, and "Games on this PC" (games DXO found
  in Steam, Epic or GOG, which are also listed first);
* **Get** downloads the setup into your Library. If another profile already
  switches on for that game, DXO asks which one should.

## Sharing a profile

1. In DXO: Profiles > Library > ... > **Export this profile**.
2. Add the file under `profiles/` in the hub repository and an entry to
   `index.json`, as a pull request.

`cargo test` checks the starter hub (`crates/dxo-core/tests/community_hub.rs`):
every entry has a game and a known category and its file loads. Hubs built
from the starter can run the same check in CI.

## Safety

A profile is a text file, but a button can be set to **open a program**. So:

* profiles downloaded from a hub always have those actions removed, and DXO
  says how many it removed;
* importing a file or restoring a backup lists exactly what would be opened
  and removes them unless you choose to keep them;
* a download is limited to 256 KB and must parse as a DXO profile.
