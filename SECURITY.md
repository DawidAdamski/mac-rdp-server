# Security

## Read this before you expose the port

**This server has no authentication.** None. There is no password, no username,
no NLA, no client certificate check. Anything that completes the TLS handshake
on the listening port gets a live view of your Mac's screen and can move your
mouse and type on your keyboard.

That is not a bug report waiting to happen — it is the current state of the
project, written down on purpose. Network-level authentication is specified
(F-101, wave 4) and not implemented yet.

Two consequences follow, and they are the whole point of this file:

1. **The default bind address is `127.0.0.1` and you should keep it that way**
   unless you understand what changing it does. On the default, only programs
   on this Mac can connect; an SSH tunnel is the safe way to reach it from
   elsewhere (see below).
2. **`--bind-addr 0.0.0.0` hands your desktop to everyone on the network.**
   Every device on the same Wi-Fi — including ones you do not control, like a
   guest's phone or a smart TV — can connect without being asked for anything.
   Use it on a network you trust, for as long as you need it, and not longer.

**Never expose this server to the internet.** Do not forward port 3389 on your
router, do not put it behind a public load balancer, do not run it on a cloud
VM with a public IP. RDP on a public address is scanned and brute-forced
continuously; with no authentication there is nothing to brute-force.

## Reaching your Mac from elsewhere, safely

Use SSH port forwarding and leave the server on `127.0.0.1`:

```sh
# on the client machine
ssh -L 3389:127.0.0.1:3389 you@your-mac
# then point the RDP client at 127.0.0.1:3389
```

SSH gives you the authentication and the encrypted transport that this server
does not.

## The TLS certificate

On first run the server generates a self-signed certificate and key in
`~/Library/Application Support/mac-rdp-server/`. The private key is written
with mode `0600` (owner only); there is a unit test asserting that.

Because the certificate is self-signed, every client will warn you that it
cannot verify the server's identity, and you will click through that warning.
That means the transport is encrypted but **not authenticated**: on a hostile
network, something in the middle can present its own certificate and you would
not be able to tell. On a home LAN that is an acceptable trade. On a café
network it is not — use the SSH tunnel instead.

## What the server can do to your Mac

Both of these are granted by you, in System Settings, and can be revoked there:

- **Screen Recording** — the server reads the contents of the display it is
  capturing. Everything visible on that display goes to the connected client,
  including whatever is on screen when you are not looking.
- **Accessibility** — the server injects keyboard and mouse events as if you
  had typed and clicked them. A connected client can do anything you can do at
  the keyboard, including authorising things with your logged-in session.

If you revoke either permission, the server degrades instead of crashing: it
serves an explanatory placeholder image, or logs input instead of injecting it.

## Logging

`MAC_RDP_LOG=trace` logs **every key** the client sends, which includes
passwords typed over the connection. That is why keystroke contents are logged
at `trace` and never at `debug` or above. Do not run at `trace` while typing
secrets, and do not paste a trace log into an issue without reading it first.

## Reporting a vulnerability

Please report security issues **privately**, not as a public issue:

- GitHub → the repository's **Security** tab → **Report a vulnerability**
  (private security advisory).

Include what you did, what happened, and which version (`mac-rdp-server
--version`). You will get an acknowledgement; this is a one-person hobby
project, so please allow some days.

Out of scope, because they are documented above rather than discovered:

- the absence of authentication;
- the self-signed certificate not being verifiable;
- the screen and input capabilities that the two macOS permissions grant;
- anything that requires the attacker to already be on your network **and**
  you to have started the server with `--bind-addr 0.0.0.0`.

In scope, and worth reporting: anything reachable on the default
`127.0.0.1` binding that a local unprivileged process should not be able to do,
memory-safety faults, a parser crash from a malformed PDU, the private key
being written readable by other users, or the server injecting input after a
client has disconnected.

## Supported versions

Only the latest release gets fixes. There are no long-term support branches.
