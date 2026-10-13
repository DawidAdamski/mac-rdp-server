# homebrew-mac-rdp-server

Homebrew tap for [**mac-rdp-server**](https://github.com/DawidAdamski/mac-rdp-server) —
an RDP server for macOS: see your Mac's screen, move its mouse and type on its
keyboard from any RDP client.

```sh
brew install DawidAdamski/mac-rdp-server/mac-rdp-server
```

Or explicitly:

```sh
brew tap DawidAdamski/mac-rdp-server
brew install mac-rdp-server
```

Then grant the two permissions macOS requires, and check they took effect:

```sh
mac-rdp-server --check-permissions
mac-rdp-server
```

> [!WARNING]
> **The server has no authentication.** It listens on `127.0.0.1` by default,
> which keeps it on your own Mac. Read
> [SECURITY.md](https://github.com/DawidAdamski/mac-rdp-server/blob/main/SECURITY.md)
> before changing that.

The formula here is generated at release time from
`packaging/homebrew/mac-rdp-server.rb` in the main repository. An edit made in
this tap is lost at the next release — change it there.

<!--
Source of truth: packaging/homebrew/tap-README.md in DawidAdamski/mac-rdp-server.
Copied into the tap by the procedure in packaging/RELEASING.md §1.5.
-->
