<p align="center">
  <img src="data/icons/io.github.ninjairocode.SnowLauncher.svg" width="128" height="128" alt="Snow, a white boxer dressed as a wizard">
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

> **Early development.** Snow Launcher currently works with a Battle.net
> prefix you already have (for example, from Lutris). There is no graphical
> interface yet, and it does not create or install anything for you.

## What it is

Playing Blizzard games on Linux usually means running Battle.net under Wine
or Proton through a generic launcher. It works, but it breaks in familiar
ways. Snow Launcher is a companion focused on that one setup:

- **No automatic suspend while you play.** While a game runs, the computer
  will not go to sleep on its own (the screen can still turn off). You can
  turn this off.
- **Warns when a game's connection dies.** After a long enough network
  outage (a suspend, a pulled cable, a router restart), the game server
  drops your session, but the game keeps running and can take around 15
  minutes to notice: you can still walk around, but nothing reaches the
  server. Snow Launcher notices in under a minute and tells you to restart
  the game.
- **Explains known errors.** `snowctl doctor` reads Battle.net's own logs
  and explains the error codes it knows, without guessing about the others.

The plan is to grow into a full manager for a dedicated Battle.net
environment, with its own Proton runtime, automatic fixes and a GNOME
interface. See the [roadmap](project.md#12--roadmap).

## What it is not

- **Not an alternative Battle.net client.** It never talks to Blizzard's
  servers or touches your credentials; you always log in through the
  official app.
- **Not a cheat or mod tool.** It does not inject code into games, read or
  change their memory, or get around anti-cheat.
- It only looks at processes of the Wine prefix it is configured for, so a
  Battle.net running elsewhere (Lutris, Steam) is never touched.

## Requirements

- Linux with systemd (logind) and NetworkManager
- A desktop with the XDG desktop portal and notifications (developed on
  GNOME / Fedora)
- An existing Wine/Proton prefix with Battle.net installed
- [Rust](https://rustup.rs/) 1.85 or newer, to build it

## Install from source

```sh
git clone https://github.com/ninjairocode/snow-launcher.git
cd snow-launcher
cargo install --locked --path crates/snowlauncher-daemon   # snowd
cargo install --locked --path crates/snowlauncher-cli      # snowctl
```

Tell it where your Battle.net prefix is, in
`~/.config/snow-launcher/config.toml`:

```toml
schema = 1
prefix = "~/Games/battlenet"

[sleep]
# Set to true to let the computer suspend automatically even while playing.
allow_suspend_while_playing = false
```

Then run the daemon as a user service, so it starts with your session:

```sh
snowctl service install --dry-run   # shows what it would do
snowctl service install
```

To remove it: `snowctl service uninstall`.

## Usage

```sh
snowctl status    # Battle.net and game processes, and what snowd is seeing
snowctl doctor    # known problems in Battle.net's recent logs
journalctl --user -u snowd -f   # the daemon's log
```

## How it works

- `snowd`, a small daemon, watches the prefix's processes, listens to
  suspend and resume (logind) and to the network (NetworkManager), asks the
  desktop not to suspend while a game runs, and reads the games' TCP
  connection state from `/proc` (no root needed).
- `snowctl` is the command-line tool.
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
