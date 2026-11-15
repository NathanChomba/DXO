# DXO community profiles

Controller profiles for DXO, shared by players.
Open DXO's **Community** page and paste this repository's raw `index.json`
address to browse them:

    https://raw.githubusercontent.com/<owner>/<repo>/main/index.json

## Add your setup

1. In DXO: Profiles > Library > ... > **Export this profile**.
2. Fork this repository, put the file in `profiles/` (lowercase, dashes:
   `game-setup.toml`), and give it a `name` that says what it's for
   ("CoD: Gyro aim", "Forza Horizon 5: Pedal feel").
3. Add an entry to `index.json` (fields in the table below) and open a pull
   request. Say which controller you tested it on.

| Field | Required | |
|---|---|---|
| `name` | yes | Same as `name` in the .toml |
| `url` | yes | `profiles/your-file.toml` |
| `game` | yes | The game's usual name, so setups group together |
| `category` | yes | Shooter, Racing, Action & adventure, Sports, Fighting, Platformer, Strategy, Flight & simulation, Emulation, Desktop, Other |
| `controller` | | "DualSense", "DualSense Edge", "DualShock 4", or "Any" |
| `author`, `description`, `tags` | | Shown and searchable in DXO |

Rules: no "open a program" actions (DXO removes them anyway), and nothing
built to give an unfair edge online (aim-assist, anti-recoil, macros that
replace skill).
