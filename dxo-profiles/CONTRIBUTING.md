# Adding a profile

Everything can be done in the browser; no Git needed.

## 1. Export it from DXO

Profiles > Library > select the profile > **...** > **Export this profile**.
Before exporting, give it a name in the form `Game: Setup`, for example
`CoD: Gyro aim` or `Forza Horizon 5: Pedal feel`, and make sure the game's
.exe is set (Games page), so it switches on by itself.

## 2. Fork this repository

Click **Fork** (top right), then **Create fork**.

## 3. Add the file

In your fork, open `profiles/`, then the game's folder (or create one:
**Add file > Create new file**, type `game-name/` and a file name).
Use **Add file > Upload files** to upload your `.toml`.
Folder and file names: lowercase, dashes, no spaces
(`profiles/call-of-duty/gyro-aim.toml`).

## 4. List it in index.json

Open `index.json`, click the pencil, and copy an existing entry inside
`"profiles": [ ... ]`. Change every field:

| Field | What to put |
|---|---|
| `id` | folder + file name with dashes: `call-of-duty-gyro-aim` |
| `name` | exactly the `name` inside your .toml |
| `game` | the game's usual name, spelled like the other setups for it |
| `category` | Shooter, Racing, Action & adventure, Sports, Fighting, Platformer, Strategy, Flight & simulation, Emulation, Desktop or Other |
| `controller` | DualSense, DualSense Edge, DualShock 4, Switch Pro or Any |
| `author` | your GitHub username |
| `description` | what it does and who it suits, under 200 characters |
| `tags` | up to 8 lowercase words, optional |
| `version` | `1` for a new profile; raise it by 1 when you change one |
| `updated` | today's date, `YYYY-MM-DD` |
| `url` | `profiles/<folder>/<file>.toml` |

Watch the commas between entries.

## 5. Open a pull request

**Contribute > Open pull request**, fill in the template, and submit. A
check runs within a minute: if it's red, its log says what to fix; edit the
files in your fork and the check runs again. A maintainer then reviews it.

## Not accepted

"Open a program" actions; aim-assist, anti-recoil, or macros that act faster
than a person in competitive online games; anything meant to get round
anti-cheat; offensive names or text; near-copies of a setup already listed
for the game (improve that one instead).

## Check locally (optional)

    pip install jsonschema
    python tools/validate.py
