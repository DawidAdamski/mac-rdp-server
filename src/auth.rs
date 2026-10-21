//! Authentication: whether a password is required, and where it comes from
//! (F-101).
//!
//! No RDP in this module, by the boundary rule in `CLAUDE.md`. It answers three
//! questions — is authentication configured, is this address reachable from
//! anywhere but this machine, and what is the secret — and `session` turns the
//! answer into `with_hybrid` and CredSSP.
//!
//! The secret lives in the Keychain and nowhere else (F-101.5, F-101.6). It is
//! deliberately impossible to pass a password on the command line or put one in
//! the configuration file: an argument ends up in the shell history and in `ps`
//! for every user of the machine, and a file ends up in a backup.

use std::io::{IsTerminal as _, Write as _};
use std::net::IpAddr;

use anyhow::{Context as _, Result};

/// Keychain service name for the one secret this server has (F-101.5).
///
/// The account on the item is the RDP username, so changing the username means
/// a different item rather than a silently reused password.
pub const KEYCHAIN_SERVICE: &str = "mac-rdp-server";

/// `errSecItemNotFound`: the item is simply not there.
///
/// Worth a constant because "no password set yet" is an ordinary state that has
/// to be told apart from a Keychain that refused or failed — the two lead to
/// different messages for the user (F-101.11).
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

/// The RDP account: a username and the secret that goes with it.
///
/// Not a macOS account. This is one secret for one purpose, checked by this
/// server (F-101 §2).
///
/// `Debug` is written by hand and prints no password at any level, `trace`
/// included (F-101.15). The engine's own `Credentials` redacts too, but this
/// type is ours and our code is what logs it; F-003a.19 is the standing
/// reminder that trace-level logging can record a password.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// What the server should do about authentication on this run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// NLA/CredSSP with these credentials (F-101.1).
    Nla(Credentials),
    /// Nothing configured. The server keeps the behaviour it has always had
    /// (F-101.4), which is why `--test-pattern` on loopback still needs no
    /// secret.
    None,
}

impl Mode {
    /// The configured username, if any. Safe to log.
    #[must_use]
    pub fn username(&self) -> Option<&str> {
        match self {
            Self::Nla(creds) => Some(&creds.username),
            Self::None => None,
        }
    }

    /// `"nla"` or `"none"`, for the startup line (F-101.16).
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Nla(_) => "nla",
            Self::None => "none",
        }
    }
}

/// Why the server refuses to start.
///
/// Two variants rather than one message, because "you did not configure this"
/// and "you configured it and the secret is gone" are different mistakes and
/// need different instructions (F-101.11).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// F-101.10: a reachable address and no password.
    #[error(
        "refusing to listen on {bind_addr} without authentication: anything that can reach that \
         address would get the screen and the keyboard. Either set a password — \
         `mac-rdp-server --auth-user NAME --set-password` — or listen on 127.0.0.1."
    )]
    NotConfigured { bind_addr: IpAddr },

    /// F-101.11: a username is configured, so authentication was meant, but
    /// there is no secret to check against. Serving without one would quietly
    /// undo the decision.
    #[error(
        "authentication is configured for user `{username}`, but the Keychain has no password \
         for it. Set one with `mac-rdp-server --auth-user {username} --set-password`, or remove \
         `auth_user` from the configuration to run without authentication on loopback."
    )]
    SecretMissing { username: String },
}

/// Whether an address can only be reached from this machine (F-101.12).
///
/// `0.0.0.0` and `::` are **not** loopback. They are "listen on every
/// interface", which is precisely the case F-101.10 exists for, and reading
/// them as loopback would turn the whole requirement into a no-op.
///
/// An IPv4-mapped IPv6 address (`::ffff:127.0.0.1`) *is* loopback: binding it
/// reaches the loopback interface, so judging it by the mapped address is what
/// matches the behaviour of the socket.
#[must_use]
pub fn is_loopback(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(mapped) => mapped.is_loopback(),
            None => v6.is_loopback(),
        },
    }
}

/// Decides the mode from the configured username, the address, and the secret
/// that was already looked up.
///
/// The Keychain read is the caller's job, which is what keeps this a pure
/// function and both refusals unit-testable (F-101.12 asks for exactly this
/// shape for the loopback decision, and the rest of the decision is no harder
/// to test than the hard part).
pub fn decide(username: Option<&str>, bind_addr: IpAddr, secret: Option<String>) -> Result<Mode, Refusal> {
    let Some(username) = username else {
        // F-101.7: no username means authentication is not configured, and
        // F-101.10 then allows only loopback.
        return if is_loopback(bind_addr) {
            Ok(Mode::None)
        } else {
            Err(Refusal::NotConfigured { bind_addr })
        };
    };

    // Deliberately not conditional on the address: a configured username is a
    // decision that a password is required, and honouring it only on public
    // addresses would mean the loopback case silently runs without the
    // authentication the configuration asks for (F-101.11).
    let Some(password) = secret else {
        return Err(Refusal::SecretMissing {
            username: username.to_owned(),
        });
    };

    Ok(Mode::Nla(Credentials {
        username: username.to_owned(),
        password,
    }))
}

/// [`decide`] with the Keychain read done for it.
///
/// The read happens only when a username is configured, so a server running
/// without authentication never touches the Keychain — and therefore never
/// causes a permission dialog for a secret it does not want.
pub fn resolve(username: Option<&str>, bind_addr: IpAddr) -> Result<Mode> {
    let secret = match username {
        Some(username) => read_secret(username)?,
        None => None,
    };
    Ok(decide(username, bind_addr, secret)?)
}

/// Reads the password for `username` from the Keychain.
///
/// `Ok(None)` means there is no such item, which is an ordinary state. Anything
/// else — a locked Keychain, a refused ACL — comes back as `Err`, because
/// treating "I could not look" as "there is nothing" would turn a refused
/// prompt into a server that starts without authentication.
pub fn read_secret(username: &str) -> Result<Option<String>> {
    match security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, username) {
        Ok(bytes) => {
            let password = String::from_utf8(bytes).with_context(|| {
                format!("the Keychain item for `{username}` does not hold valid UTF-8; set the password again")
            })?;
            Ok(Some(password))
        }
        Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
        Err(error) => Err(anyhow::Error::new(error).context(format!(
            "reading the password for `{username}` from the Keychain (service `{KEYCHAIN_SERVICE}`)"
        ))),
    }
}

/// Stores the password for `username`, replacing any previous one.
pub fn write_secret(username: &str, password: &str) -> Result<()> {
    security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, username, password.as_bytes()).with_context(
        || format!("storing the password for `{username}` in the Keychain (service `{KEYCHAIN_SERVICE}`)"),
    )
}

/// Reads a password and stores it, printing nothing that reveals it (F-101.8).
///
/// From a terminal the input is not echoed; from anything else it is read as a
/// line on standard input, so that setting a password can be scripted without
/// the secret ever becoming a process argument (F-101.6).
pub fn set_password_interactively(username: &str) -> Result<()> {
    let password = read_new_password(username)?;
    let password = validate_new_password(&password)?;
    write_secret(username, password)?;

    // Says what happened and names nothing secret. Printed rather than logged:
    // this is a mode the user ran on purpose and is waiting on, not a server
    // event.
    println!("Password stored in the Keychain for user `{username}` (service `{KEYCHAIN_SERVICE}`).");
    Ok(())
}

/// Rejects a password that would lock the server open (F-101.9).
///
/// Separate and pure so the rule is testable without a terminal. Whitespace is
/// trimmed from both ends: a trailing newline from a pipe is an artefact of how
/// the password arrived, not part of it, and a password that is only whitespace
/// is empty for every purpose that matters here.
pub fn validate_new_password(password: &str) -> Result<&str> {
    let trimmed = password.trim();
    if trimmed.is_empty() {
        anyhow::bail!("an empty password is not stored: it would be the same as no authentication");
    }
    Ok(trimmed)
}

fn read_new_password(username: &str) -> Result<String> {
    if std::io::stdin().is_terminal() {
        return rpassword::prompt_password(format!("New RDP password for `{username}` (not echoed): "))
            .context("reading the password from the terminal");
    }

    // Not a terminal: one line on standard input. Prompting would only pollute
    // the output of whatever script is driving this.
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .context("reading the password from standard input")?;
    std::io::stdout().flush().ok();
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(text: &str) -> IpAddr {
        text.parse().expect("valid test address")
    }

    /// F-101.12: the two addresses the requirement names explicitly. Getting
    /// these backwards is the whole failure mode — `0.0.0.0` read as loopback
    /// would let the server onto the network with no password at all.
    #[test]
    fn f101_12_listen_on_everything_is_not_loopback() {
        assert!(
            !is_loopback(addr("0.0.0.0")),
            "0.0.0.0 is every interface, not loopback"
        );
        assert!(!is_loopback(addr("::")), ":: is every interface, not loopback");
    }

    #[test]
    fn f101_12_loopback_addresses_are_loopback() {
        assert!(is_loopback(addr("127.0.0.1")));
        assert!(is_loopback(addr("127.0.0.53")), "the whole 127.0.0.0/8 is loopback");
        assert!(is_loopback(addr("::1")));
        assert!(
            is_loopback(addr("::ffff:127.0.0.1")),
            "an IPv4-mapped loopback address reaches the loopback interface"
        );
    }

    #[test]
    fn f101_12_routable_addresses_are_not_loopback() {
        assert!(!is_loopback(addr("192.168.1.10")));
        assert!(!is_loopback(addr("10.0.0.1")));
        assert!(!is_loopback(addr("8.8.8.8")));
        assert!(!is_loopback(addr("fe80::1")));
        assert!(!is_loopback(addr("::ffff:192.168.1.10")));
    }

    /// F-101.4, AC-6: without a secret, loopback keeps working exactly as it
    /// did — otherwise every diagnostic run would need a password.
    #[test]
    fn f101_4_loopback_without_a_username_needs_no_secret() {
        assert_eq!(decide(None, addr("127.0.0.1"), None), Ok(Mode::None));
        assert_eq!(decide(None, addr("::1"), None), Ok(Mode::None));
    }

    /// F-101.10, AC-5: the criterion of wave 4.
    #[test]
    fn f101_10_a_reachable_address_without_a_username_is_refused() {
        let bind_addr = addr("0.0.0.0");
        assert_eq!(decide(None, bind_addr, None), Err(Refusal::NotConfigured { bind_addr }));
        assert_eq!(
            decide(None, addr("192.168.1.10"), None),
            Err(Refusal::NotConfigured {
                bind_addr: addr("192.168.1.10")
            })
        );
    }

    /// F-101.10: the message has to say what to do, not only that it will not
    /// start. Both ways out are named, because which one is right depends on
    /// what the user was trying to do.
    #[test]
    fn f101_10_the_refusal_says_what_to_do() {
        let message = Refusal::NotConfigured {
            bind_addr: addr("0.0.0.0"),
        }
        .to_string();
        assert!(message.contains("0.0.0.0"), "names the address: {message}");
        assert!(message.contains("--set-password"), "names the way out: {message}");
        assert!(message.contains("127.0.0.1"), "names the other way out: {message}");
    }

    /// F-101.11: configured but no secret is its own refusal, with its own
    /// message. "You did not set this up" and "you set it up and the password
    /// is gone" need different instructions.
    #[test]
    fn f101_11_a_configured_username_without_a_secret_is_refused() {
        assert_eq!(
            decide(Some("rdp"), addr("0.0.0.0"), None),
            Err(Refusal::SecretMissing {
                username: "rdp".to_owned()
            })
        );
    }

    /// F-101.11: and on loopback too. A configured username is a decision that
    /// a password is required; honouring it only on public addresses would let
    /// the loopback case run without the authentication that was asked for.
    #[test]
    fn f101_11_a_missing_secret_is_refused_on_loopback_as_well() {
        assert_eq!(
            decide(Some("rdp"), addr("127.0.0.1"), None),
            Err(Refusal::SecretMissing {
                username: "rdp".to_owned()
            })
        );
    }

    #[test]
    fn f101_11_the_two_refusals_do_not_share_a_message() {
        let not_configured = Refusal::NotConfigured {
            bind_addr: addr("0.0.0.0"),
        }
        .to_string();
        let secret_missing = Refusal::SecretMissing {
            username: "rdp".to_owned(),
        }
        .to_string();
        assert_ne!(not_configured, secret_missing);
        assert!(
            secret_missing.contains("rdp"),
            "the second one names the user it looked for: {secret_missing}"
        );
    }

    /// F-101.1, F-101.3: a username with a secret is NLA, and the username
    /// travels with it so the audit log can name it.
    #[test]
    fn f101_1_a_username_with_a_secret_enables_nla() {
        let mode = decide(Some("rdp"), addr("0.0.0.0"), Some("s3cret".to_owned())).expect("configured");
        assert_eq!(
            mode,
            Mode::Nla(Credentials {
                username: "rdp".to_owned(),
                password: "s3cret".to_owned(),
            })
        );
        assert_eq!(mode.username(), Some("rdp"));
        assert_eq!(mode.label(), "nla");
    }

    #[test]
    fn f101_16_the_mode_has_a_label_for_the_startup_line() {
        assert_eq!(Mode::None.label(), "none");
        assert_eq!(Mode::None.username(), None);
    }

    /// F-101.15: the one thing that must never be printable. `{:?}` is what
    /// `tracing` uses for a field recorded with `?`, so this is the exact
    /// formatting a stray `trace!(?credentials, ...)` would reach for.
    #[test]
    fn f101_15_debug_never_shows_the_password() {
        let creds = Credentials {
            username: "rdp".to_owned(),
            password: "correct-horse-battery-staple".to_owned(),
        };
        let shown = format!("{creds:?}");
        assert!(
            !shown.contains("correct-horse-battery-staple"),
            "the password must not be formattable: {shown}"
        );
        assert!(shown.contains("rdp"), "the username is not a secret: {shown}");
    }

    /// F-101.15 again, through [`Mode`], which is the type that actually gets
    /// carried around and is therefore the one at risk of being logged.
    #[test]
    fn f101_15_debug_of_the_mode_never_shows_the_password() {
        let mode = Mode::Nla(Credentials {
            username: "rdp".to_owned(),
            password: "correct-horse-battery-staple".to_owned(),
        });
        let shown = format!("{mode:?}");
        assert!(!shown.contains("correct-horse-battery-staple"), "{shown}");
    }

    /// F-101.9: an empty password is the same as no authentication, so storing
    /// it would be storing a lie.
    #[test]
    fn f101_9_an_empty_password_is_not_stored() {
        assert!(validate_new_password("").is_err());
        assert!(validate_new_password("   ").is_err(), "whitespace only is still empty");
        assert!(validate_new_password("\n").is_err());
    }

    /// A password arriving down a pipe carries the newline that ended the line;
    /// that is how it was delivered, not part of the secret.
    #[test]
    fn f101_8_a_trailing_newline_is_not_part_of_the_password() {
        assert_eq!(validate_new_password("hunter2\n").expect("valid"), "hunter2");
        assert_eq!(validate_new_password(" hunter2 ").expect("valid"), "hunter2");
    }

    #[test]
    fn f101_5_the_keychain_service_is_the_one_the_spec_names() {
        assert_eq!(KEYCHAIN_SERVICE, "mac-rdp-server");
    }
}
