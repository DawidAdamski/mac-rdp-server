# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/).

## Versioning

This project follows [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html),
with the exception that **`0.y.z` is allowed to break anything on a `y` bump** —
and `0.y.z` is where the project is today. Treat every minor release before
`1.0.0` as a release that may change behaviour, configuration keys or the
command line without a migration path.

`version` in `Cargo.toml` is the single source of truth. A tag is that number
with a `v` prefix, and the release workflow refuses a tag that disagrees with
the manifest, or one that has no section in this file.

## [Unreleased]

### Fixed

- A display sleep no longer freezes the screen for good. ScreenCaptureKit was
  being asked to start and stop capture through a call that waited for a
  callback with no time limit; when the display slept the callback never came
  and the recovery path parked there for the life of the process, so
  reconnecting showed the same frozen frame. Capture now resumes within a few
  seconds of the display waking (F-007.19 … F-007.25, ADR-0016).
- Shutdown and startup can no longer hang on the same call, and a failed
  recovery attempt now says so in the log instead of leaving silence.
- The Mac's display no longer idles to sleep underneath a live RDP session. It
  did, and because a sleeping display cannot be captured, the screen went dead
  for as long as the display stayed off — measured at 10 minutes 50 seconds on
  a real session. The server now holds the display awake for as long as at
  least one client is connected, and releases it when the last one leaves
  (F-007.26, F-007.28, F-007.29).
- While capture is unavailable, the client is now shown a placeholder saying so
  instead of the last frame it received. A frozen picture with no explanation
  is indistinguishable from a hung server (F-007.27).
- A capture stream that stops delivering without reporting an error no longer
  freezes the screen for good. The watchdog needed rejected frames to notice
  anything, and this failure produces none — not a frame, not an idle frame,
  not a reject — so nothing was detected and reconnecting could not help,
  because the stream is shared by the whole process. Measured at thirteen
  minutes of frozen screen; now the stream is rebuilt about six seconds after
  it falls silent (F-007.30 … F-007.34).

### Added

- A menu-bar mode: `mac-rdp-server --menu-bar` puts an icon in the menu bar
  with the address, the state of both permissions, and a switch that starts and
  stops the server without restarting the program. The process stays out of the
  Dock (F-111).
- In that mode the server starts **stopped**: running the program is not
  running the service, and while the service is off nothing captures the screen
  either — so the system's screen-recording indicator stays off too
  (F-111.18, F-111.19).
- A failed start says why. A busy port, a missing certificate or a refused
  permission leaves the switch off, turns the icon into a warning triangle and
  puts the reason in the first line of the menu instead of failing silently
  where no terminal can see it (F-111.11).
- Settings can be given once instead of at every start: the server reads
  `~/.config/mac-rdp-server.toml` without being pointed at it. Precedence is
  command line, then the file named by `--config`, then that default file, then
  the defaults. `--config` *replaces* the default file rather than adding to
  it, so one file is read or the other, never both (F-112.1, F-112.2).
- The startup log line now says where the configuration came from — the path of
  the file that was read, or `built-in defaults` — so "where did that value
  come from" is answered by reading the log rather than by experiment
  (F-112.7).
- `--no-config` ignores the default file, which is the way past one that stops
  the server from starting without having to delete it. A default file that
  exists but cannot be read or parsed is an error with the path in the message;
  a missing one is normal and silent (F-112.4, F-112.5, F-112.6).
- `capture stats` reports `silent_for_ms`, the time since ScreenCaptureKit last
  called us at all, so a frozen stream is readable from a single log line
  instead of by diffing two lines ten seconds apart (F-007.31).

### Changed

- `main` no longer owns the service. Everything the server needs — capture,
  input, the display wake lock, the socket — is built, run and torn down by one
  module, because the menu bar has to do that many times in one process where
  `main` only ever did it once. CLI mode is unchanged from the outside, and an
  integration test on the real binary now holds that: same startup log, same
  address, `Ctrl-C` still exits zero (F-111 AC-3).
- The Homebrew caveat used to show an example configuration being copied to
  `~/.config/mac-rdp-server.toml` and stop there, as if that path were read. It
  was not, and the caveat was first corrected to show the `--config` command as
  well. With that path now read on its own, the copy is genuinely all it takes
  and the caveat says so, along with where the startup log reports the file it
  used (F-112.8).
- The README and the documentation page now carry the display-sleep freeze as a
  known bug, with the two commands that work around it, instead of leaving it
  for whoever hits it to rediscover (F-007 §8c, ADR-0016).

## [0.1.1] — 2026-10-14

A patch release with one reason: the TLS fix below. Nothing about how the
program works changed.

### Security

- Updated `rustls` to 0.23.45, which fixes GHSA-2mjx-qc3c-rqvc: a TLS 1.3
  server accepted handshake messages across encryption level boundaries. The
  server's TLS comes from `rustls` by way of `ironrdp-server`, so this applies
  to the server itself and not to some unused code path. **The `0.1.0`
  binaries carry the affected version (0.23.42); replace them with this
  release.** The same update brings `aws-lc-rs` 1.18.2, `aws-lc-sys` 0.46.0
  and `rustls-webpki` 0.103.15 along with it, which 0.23.45 requires.

### Changed

- `--check-permissions` no longer prints an internal requirement number at the
  reader. When both permissions read as granted it now says what to actually
  do if the server still does nothing: remove the stale entry with the minus
  button, add the current one, and start the server again.

## [0.1.0] — 2026-10-13

First release. A development release: it works, and it is not finished. Read
the warnings before exposing it to anything.

### Added

- **Screen** — the live screen over RDP, captured with ScreenCaptureKit, with
  damage tracking so that only what changed is sent. A `--test-pattern
  WIDTHxHEIGHT` mode serves a synthetic picture instead, which separates
  "capture is wrong" from "the protocol path is wrong" without a rebuild.
- **Input** — mouse and keyboard injected with CGEvent: left, right and middle
  button, both scroll axes, and a keyboard mapping that holds modifiers,
  Caps Lock and dead keys across focus changes. Unicode input is accepted
  from clients that send it.
- **Protocol** — TLS on port 3389, a self-signed certificate generated on
  first run with the key written `0600`, Suppress Output and Refresh Rect
  handled so that a minimised or restored window does not leave a stale
  picture, and a repaint on reactivation instead of a disconnect.
- **Permissions** — `--check-permissions` prints whether Screen Recording and
  Accessibility are granted, says what to click for the ones that are not, and
  exits non-zero so a script can gate on it. The instructions name the case
  that actually bites: the stale entry left in the list after an upgrade.
- **Configuration** — a TOML file, overridable per key by command-line flags,
  which win. An unknown key in the file is an error, not a silent no-op.
- **Distribution** — a Homebrew tap, a prebuilt binary for Apple Silicon and
  Intel, a `--HEAD` path that builds from source, `README.md`, `SECURITY.md`,
  both licences, and a project page generated from the Markdown rather than
  maintained beside it.

### Known limitations

- **There is no authentication.** Anything that can reach the port gets the
  screen and the keyboard. The default binding is `127.0.0.1`. See
  `SECURITY.md` before changing it.
- No clipboard, no audio, and only one display is served.
- The picture does not follow a resize initiated by the client.
- Emoji reach the Mac only from clients that send Unicode keyboard events;
  `mstsc` does not.
- Tested on macOS 26 on Apple Silicon. Intel is built but untested; macOS 14
  is the lowest version the APIs allow and has not been tried.
- The RDP engine is [a fork of IronRDP](https://github.com/DawidAdamski/IronRDP)
  carrying two patches that are not upstream yet: the middle mouse button, and
  tolerating queued data during reactivation.

[Unreleased]: https://github.com/DawidAdamski/mac-rdp-server/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/DawidAdamski/mac-rdp-server/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/DawidAdamski/mac-rdp-server/releases/tag/v0.1.0
