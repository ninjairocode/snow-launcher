<p align="center">
  <img src="data/icons/io.github.ninjairocode.SnowLauncher.svg" width="128" height="128" alt="Snow, a white boxer in a wizard hat, peeking out of his doghouse">
</p>

<h1 align="center">Snow Launcher</h1>

<p align="center">
  Keeps the official Battle.net app well configured and watched over on Linux.
</p>

<p align="center">
  <a href="https://github.com/ninjairocode/snow-launcher/actions/workflows/ci.yml"><img src="https://github.com/ninjairocode/snow-launcher/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue" alt="License: GPL-3.0-or-later">
  <img src="https://img.shields.io/badge/AI--assisted-yes-8a2be2" alt="AI-assisted project">
</p>

<p align="center"><a href="README.pt-BR.md">Leia em português</a></p>

---

> **Early development (v0.3).** Snow Launcher sets up and watches over its
> own Battle.net environment, with a GNOME window and a tray icon. It works
> on the maintainer's machine; expect rough edges elsewhere.

## What it is

Playing Blizzard games on Linux usually means running Battle.net under Wine
or Proton through a generic launcher. It works, but it breaks in familiar
ways. Snow Launcher is a companion focused on that one setup:

- **Its own Battle.net "house".** A Wine prefix made for Battle.net with
  [umu](https://github.com/Open-Wine-Components/umu-launcher) and the Proton
  you choose (GE-Proton by default, any build ProtonPlus installed), set to
  Windows 11 and to your desktop's scale, so Battle.net is readable.
- **Installs Battle.net safely.** It never ships or downloads Blizzard's
  installer: you download it in your browser, and Snow checks Blizzard's
  digital signature before running it.
- **Brings your games.** Imports a game from Lutris or Steam into its library,
  as an instant copy on Btrfs/XFS; the original keeps working.
- **One click to play.** A window and a tray icon with your games (their icons
  come from your own game files) and Battle.net's startup progress.
- **No automatic suspend while you play**, and a **warning when a game's
  connection dies** after an outage, instead of the game silently staying
  offline for about 15 minutes.
- **Optional MangoHud and GameMode**, using the ones you installed.
- **Safe by design:** dry runs, backups before changes, a way back for
  everything, and it never closes a running game.

## What it is not

- **Not an alternative Battle.net client.** It never talks to Blizzard's
  servers or touches your credentials; you always log in through the
  official app.
- **Not a cheat or mod tool.** It does not inject code into games, read or
  change their memory, or get around anti-cheat.
- It only touches processes of its own Wine prefix, identified by
  `WINEPREFIX`, so a Battle.net running elsewhere (Lutris, Steam) is never
  touched.

## Requirements

- Linux with systemd (logind) and NetworkManager
- GNOME with the AppIndicator extension for the tray icon (developed on
  Fedora; the minimum base is Ubuntu 24.04: GTK 4.14, libadwaita 1.5)
- [umu-launcher](https://github.com/Open-Wine-Components/umu-launcher) (or
  Lutris, which bundles it) and a GE-Proton build (for example, from
  ProtonPlus)
- [Rust](https://rustup.rs/) 1.85 or newer, plus the GTK 4 and libadwaita
  development packages, to build it

## Install from source

```sh
git clone https://github.com/ninjairocode/snow-launcher.git
cd snow-launcher
cargo install --locked --path crates/snowlauncher-daemon   # snowd
cargo install --locked --path crates/snowlauncher-cli      # snowctl
cargo install --locked --path crates/snowlauncher-gui      # the window
snowctl service install   # snowd as a user service, started with your session
```

## Getting started

```sh
snowctl house create --dry-run    # shows what it would do
snowctl house create              # the house: prefix, Windows 11, scale
```

Then open **Snow Launcher**: download Battle.net's installer in your browser
("Open download page"), press **Install Battle.net**, log in, and import or
install your games. Everything is also in `snowctl` (`snowctl --help`):

```sh
snowctl status            # what snowd is seeing
snowctl games             # the library's games
snowctl play wowf         # open a game through Battle.net
snowctl import            # games other launchers have
snowctl doctor            # known problems in Battle.net's recent logs
```

## How it works

- `snowd`, a small daemon, watches the house's processes, listens to suspend
  and resume (logind) and to the network (NetworkManager), asks the desktop
  not to suspend while a game runs, reads the games' TCP connection state
  from `/proc` (no root needed), and shows the tray icon.
- `snowctl` is the command-line tool; `snow-launcher` is the GNOME window
  (Relm4 and libadwaita).
- They talk over D-Bus, on the session bus
  (`io.github.ninjairocode.SnowLauncher1`).

The design, the decisions and their reasons are in [`project.md`](project.md)
(Portuguese). Measurements from real use are in
[`docs/experimentos.md`](docs/experimentos.md).

## AI-assisted project

Snow Launcher is developed with the help of Claude (Anthropic). Every change
is reviewed, tested and committed by a human maintainer, and commits that
had significant AI help carry an `Assisted-by:` trailer.

## Not affiliated with Blizzard

Snow Launcher is an independent project. It is not affiliated with, endorsed
by or sponsored by Blizzard Entertainment. Battle.net, World of Warcraft and
the other names mentioned are trademarks of their respective owners.

## License

[GPL-3.0-or-later](LICENSE).

---

<sub>For Snow: white boxer, caramel patch over one eye, crooked smile, a wink
on the side. Now the project's official wizard.</sub>
