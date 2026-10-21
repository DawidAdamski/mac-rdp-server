# mac-rdp-server

An RDP server for macOS, written in Rust. Point Microsoft Remote Desktop — or
any other RDP client — at your Mac and see its screen, move its mouse and type
on its keyboard.

> [!WARNING]
> **There is no authentication.** Anything that can reach the port gets your
> screen and your keyboard. The default binding is `127.0.0.1`, which keeps it
> on this Mac. Read [SECURITY.md](SECURITY.md) **before** you change that, and
> never forward the port from the internet.

## What it is

- A screen-sharing server for macOS that speaks RDP, so the client you already
  have on Windows works without installing anything.
- Live screen capture through ScreenCaptureKit, with only the changed parts of
  the screen sent (damage tracking).
- Keyboard and mouse injection through CGEvent: all five mouse buttons, the
  scroll wheel, modifiers, the numeric keypad and Unicode text.
- Spec-driven and deliberately small. Every feature has a written requirement
  under [`specs/`](specs/) before it has code.

## What it is not

- **Not multi-user.** It shares the screen of the logged-in user, like screen
  sharing, not a separate session per client like Windows Terminal Services.
- **Not authenticated** (yet — that is wave 4 in [`specs/waves.md`](specs/waves.md)).
- **Not a drop-in replacement for Apple Screen Sharing.** It exists because
  RDP clients are everywhere and VNC clients on Windows are not.
- **Not finished.** See [what does not work yet](#what-does-not-work-yet).

## Requirements

| | |
|---|---|
| macOS | **14 (Sonoma) or newer** — the ScreenCaptureKit APIs used are gated on 14.0 |
| Hardware | Apple Silicon and Intel. Only Apple Silicon has actually been tested |
| Tested on | macOS 26 on Apple Silicon |
| Clients tested | `mstsc` on Windows 11, and the FreeRDP family (Windows App, `xfreerdp`) |

## Install

```sh
brew install DawidAdamski/mac-rdp-server/mac-rdp-server
```

Or, spelled out:

```sh
brew tap DawidAdamski/mac-rdp-server
brew install mac-rdp-server
```

Or without Homebrew: the
[latest release](https://github.com/DawidAdamski/mac-rdp-server/releases/latest)
holds a flat archive with the binary, an example configuration and these
notes. You can also [build it yourself](#build-from-source).

Then grant the two permissions below — the server will not capture anything or
inject anything until you do.

## Grant the two permissions

macOS will not let any program read the screen or fake a keystroke until you
say so, one permission at a time, by hand. There is no way around this and no
installer that can do it for you.

### 1. Screen Recording

**System Settings → Privacy & Security → Screen & System Audio Recording**

Enable the program you start the server *from*. If you run `mac-rdp-server` in
Terminal or iTerm, the entry macOS lists is **Terminal** or **iTerm**, not
`mac-rdp-server` — a command-line tool inherits the permissions of the
application that launched it.

The first run pops up the system dialog by itself. If you dismissed it, open
the panel above and add the entry with the **+** button.

### 2. Accessibility

**System Settings → Privacy & Security → Accessibility**

Same rule: enable the terminal application you launch the server from.

This one matters more than it looks. Without Accessibility, `CGEventPost`
silently does nothing — no error, no warning from macOS, mouse and keyboard
just do not move. The server checks on startup and tells you, which is the
only reason you are not left guessing.

### 3. Restart the server

**TCC permission changes do not reach a running process.** Quit the server and
start it again. The same goes for the terminal application: if you granted the
permission to iTerm, quit iTerm entirely (⌘Q, not just the window) and reopen
it.

### When it stops working after an update

This is the one real trap, and it is not our bug — it is the price of shipping
an ad-hoc signed command-line tool instead of a notarised app:

> After `brew upgrade mac-rdp-server`, **the permissions have to be granted
> again.** macOS keys a permission to the exact binary it was granted to, and
> Homebrew installs each version in its own directory. The new binary is, as far
> as macOS is concerned, a different program.

The confusing part: **the old entry stays in the list and can still look
switched on.** Do not trust the switch, trust the behaviour. If the screen is
black or input does nothing right after an upgrade:

1. open the panel (Screen Recording or Accessibility),
2. select the stale entry and remove it with the **−** button,
3. add the current one with **+**, or just start the server and let it ask,
4. restart the server.

The same thing happens when your *terminal application* updates itself —
iTerm's auto-update is a reliable way to lose the grant.

`yabai` and `skhd` have the identical problem for the identical reason. The
only real fix is a signed, notarised `.app` bundle in a fixed location, which
is on the roadmap (F-106) and needs a paid Apple Developer account.

## Run it

```sh
mac-rdp-server
```

That listens on `127.0.0.1:3389`, captures display 0 and generates a
self-signed TLS certificate in `~/Library/Application Support/mac-rdp-server/`
on first run.

To let other machines on your network connect:

```sh
mac-rdp-server --bind-addr 0.0.0.0
```

**Read [SECURITY.md](SECURITY.md) first.** There is no password: on
`0.0.0.0`, every device on your Wi-Fi can take over this Mac. On a network you
do not fully trust, use an SSH tunnel instead and leave the binding alone.

Useful while setting up:

```sh
mac-rdp-server --check-permissions          # what is granted, and what to click
mac-rdp-server --test-pattern 1280x720      # serve a synthetic image, not the screen
MAC_RDP_LOG=debug mac-rdp-server            # verbose logs
```

## Connect

Accept the certificate warning when the client complains it cannot verify the
server — the certificate is self-signed, which is expected and explained in
[SECURITY.md](SECURITY.md).

**Windows** — the built-in Remote Desktop Connection (`mstsc`):

```
mstsc /v:your-mac.local:3389
```

**macOS / iOS** — Windows App (formerly Microsoft Remote Desktop) from the App
Store. Add a PC with the hostname `your-mac.local`, leave the user account
empty.

**Linux** — FreeRDP:

```sh
xfreerdp /v:your-mac.local:3389 /cert:ignore -nla
```

`-nla` matters: the server offers TLS only, not Network Level Authentication.

## Configuration

Every option can be given on the command line or in a TOML file. The server
reads `~/.config/mac-rdp-server.toml` by itself, so settings are given once
rather than typed at every start:

```toml
# ~/.config/mac-rdp-server.toml   — read without being asked
bind_addr = "127.0.0.1"
port      = 3389
display   = 0          # index, in the order `mac-rdp-server` logs at startup
max_fps   = 30
log_level = "info"
encoder   = "damage"   # "damage" sends only what changed, "full" sends whole frames
```

Precedence is **command line > the file named by `--config` > that default
file > the defaults**. `--config` *replaces* the default file instead of adding
to it: one file is read, or the other, never both.

A missing default file is normal and silent. One that exists but cannot be read
or parsed stops the server with a message naming the file — a typo in it is not
worth a day of wondering where a setting went. `--no-config` gets past such a
file without deleting it.

The startup log line says which of the two it was, so "where did that value
come from" never needs an experiment:

```
INFO starting mac-rdp-server config=/Users/you/.config/mac-rdp-server.toml addr=0.0.0.0:33891 ...
INFO starting mac-rdp-server config=built-in defaults addr=127.0.0.1:3389 ...
```

> [!NOTE]
> Reading that file without being asked is **not in a release yet**. On 0.1.1 —
> the version the tap installs — the path is ignored unless you point at it:
> `mac-rdp-server --config ~/.config/mac-rdp-server.toml`.

`mac-rdp-server --help` lists every option. `MAC_RDP_LOG` overrides
`log_level`, so a bad config file can always be overridden from the shell.

## What does not work yet

| | |
|---|---|
| Authentication | Not implemented — F-101, wave 4 |
| Clipboard | Not implemented; copy and paste do not cross the connection — F-102, wave 6 |
| Audio | Not implemented — F-103, wave 7 |
| Several Mac displays at once | One display per connection, chosen with `--display` — F-105 |
| Emoji and other non-keyboard text | The server handles Unicode input; `mstsc` never sends it. A client that sends Unicode events works |
| Resizing the client window | The desktop stays the size of the Mac's display; the client scales or scrolls |
| A menu-bar icon, a launchd service, an `.app` bundle | F-109, F-111, F-106 — wave 5 |
| **Surviving a display sleep** | Fixed, but **not in a release yet**: 0.1.1 — the version the tap installs — still freezes for good when the display sleeps. See [the picture freezes](#the-picture-freezes-and-reconnecting-does-not-help) |

## Troubleshooting

### The picture freezes and reconnecting does not help

**This affects 0.1.1, which is the version the tap installs.** It is fixed in
the source, and the fix ships in the next release.

On 0.1.1, once the Mac's display goes to sleep, capture stops and never comes
back. Reconnecting shows the same frozen frame, because the capture stream is
started once per process and every connection shares it. The cause was a call
into ScreenCaptureKit that waited for a callback with no time limit: when the
display slept the callback never arrived and the recovery path parked there for
the life of the process (F-007 section 8c, ADR-0016).

On 0.1.1, two things help:

```sh
# Keep the display awake for as long as the server runs.
caffeinate -d mac-rdp-server --bind-addr 127.0.0.1

# If it has already frozen, kill it BEFORE starting again.
pkill -9 mac-rdp-server
```

That `pkill` is not housekeeping. While a frozen server is still running it
blocks screen capture for the whole machine, so the next server you start will
hang too — with no message at all.

Measured after the fix: frames resume within about four seconds of the display
waking, and the log says `capture recovered`. If you build from source you can
check it yourself with `zsh specs/probes/f007-ac3-displaysleep/run.sh` — it
blanks the display for 25 seconds on purpose and prints PASS or FAIL.

**Black screen, or a picture saying the permission is missing.** Screen
Recording is not granted to the application you started the server from, or it
was granted before the last update. See
[when it stops working after an update](#when-it-stops-working-after-an-update).

**The picture is live but the mouse and keyboard do nothing.** Accessibility,
same story. The log says `input will be logged, not injected, until
Accessibility is granted`.

**The client connects and immediately drops.** Run with `MAC_RDP_LOG=debug` and
look for the last message before the disconnect. `IRONRDP_LOG=trace` adds the
protocol engine's own view.

**`xfreerdp` refuses to connect.** Add `-nla` and `/cert:ignore`.

**The server refuses to start, complaining about the desktop width.** RDP
bitmap encoding needs a width that is a multiple of 4. Pick another display
with `--display`, or test the rest with `--test-pattern`.

## Build from source

```sh
git clone https://github.com/DawidAdamski/mac-rdp-server.git
cd mac-rdp-server
cargo build --release
```

Needs a Rust toolchain (edition 2024, `rust-version = "1.89"`) and either Xcode
or the Command Line Tools. The protocol engine comes from
[a fork of IronRDP](https://github.com/DawidAdamski/IronRDP) pinned by commit —
see [ADR-0014](specs/adr/) for why.

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

The macOS-specific parts (capture, injection, permissions) cannot be tested
without a desktop session and the two permissions, so every spec that touches
them carries a manual checklist in its acceptance criteria.

## Versions

[`CHANGELOG.md`](CHANGELOG.md) says what changed in each release, and what each
one still cannot do. Numbering is Semantic Versioning, with the usual `0.y.z`
caveat: **a minor release before `1.0.0` may change behaviour, configuration
keys or the command line without a migration path.**

## How this project is built

Specification first: nothing gets implemented without a numbered, approved
requirement, and every commit names the requirement it serves. The specs and
the architecture decision records in [`specs/`](specs/) are in Polish; the
code, the comments and the documentation are in English.

## Licence

MIT or Apache-2.0, at your option — see [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE).
