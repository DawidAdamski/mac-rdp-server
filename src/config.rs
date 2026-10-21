//! Configuration: CLI flags over the file named by `--config` over the
//! default file over built-in defaults (F-001.4, F-112.2).

use std::net::IpAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use clap::{Parser, ValueEnum};
use serde::Deserialize;

/// Which encoder turns frames into display updates (F-004.16).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EncoderKind {
    /// Send only what changed (F-004). The default.
    #[default]
    Damage,
    /// Send every frame whole, as waves 0–2 did.
    ///
    /// Kept as a diagnostic escape hatch, not as a supported mode: when the
    /// picture looks wrong, this separates "damage tracking is wrong" from
    /// "the path to the client is wrong" without a rebuild — the same split
    /// that `--test-pattern` gives for capture.
    Full,
}

pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 3389;
pub const DEFAULT_DISPLAY: u32 = 0;
pub const DEFAULT_MAX_FPS: u32 = 30;
pub const DEFAULT_LOG_LEVEL: &str = "info";

/// The one file the server reads without being told to (F-112.1), relative
/// to the home directory.
///
/// There is deliberately no search list. A single path is what makes "where
/// did this value come from" answerable by reading one line of the log
/// instead of by experiment — which is how the 2026-10-15 measurement had
/// to be done.
pub const DEFAULT_CONFIG_FILE: &str = ".config/mac-rdp-server.toml";

// Command line interface. Not a doc comment: clap would put it in `--help`.
//
// Every option is `Option<_>` so that "not given on the command line" is
// distinguishable from "given, and happens to equal the default" — otherwise
// flags could not take precedence over the file.
#[derive(Debug, Parser, Default)]
#[command(name = "mac-rdp-server", version, about = "RDP server for macOS")]
pub struct Cli {
    /// Path to a TOML configuration file. Given, it replaces the default
    /// file rather than adding to it.
    #[arg(short, long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Ignore the default configuration file (~/.config/mac-rdp-server.toml).
    ///
    /// The way past a file that stops the server from starting, without having
    /// to delete it (F-112.6). Mutually exclusive with --config, because
    /// "ignore the default file" and "read this one instead" are two answers
    /// to the same question.
    #[arg(long, conflicts_with = "config")]
    pub no_config: bool,

    /// Address to listen on [default: 127.0.0.1]
    #[arg(long, value_name = "IP")]
    pub bind_addr: Option<IpAddr>,

    /// Port to listen on [default: 3389]
    #[arg(long, value_name = "PORT")]
    pub port: Option<u16>,

    /// TLS certificate in PEM format; generated on first run if absent
    #[arg(long, value_name = "PATH")]
    pub cert_path: Option<PathBuf>,

    /// TLS private key in PEM format; generated on first run if absent
    #[arg(long, value_name = "PATH")]
    pub key_path: Option<PathBuf>,

    /// Index of the display to capture [default: 0]
    #[arg(long, value_name = "INDEX")]
    pub display: Option<u32>,

    /// Upper bound on frames sent per second [default: 30]
    #[arg(long, value_name = "FPS")]
    pub max_fps: Option<u32>,

    /// Log level: error, warn, info, debug, trace [default: info]
    #[arg(long, value_name = "LEVEL")]
    pub log_level: Option<String>,

    /// Serve the synthetic test pattern at the given size instead of the real
    /// screen, e.g. `--test-pattern 5120x1440`. Diagnostic: it separates
    /// "capture is wrong" from "the protocol path is wrong".
    #[arg(long, value_name = "WIDTHxHEIGHT")]
    pub test_pattern: Option<String>,

    /// Encoder: damage (only what changed) or full (every frame whole)
    /// [default: damage]
    #[arg(long, value_name = "KIND")]
    pub encoder: Option<EncoderKind>,

    /// Print whether Screen Recording and Accessibility are granted, say what
    /// to click for the ones that are not, and exit. Exits non-zero if any is
    /// missing, so a script can gate on it. Checks only: it never prompts.
    #[arg(long)]
    pub check_permissions: bool,
}

/// The same keys as [`Cli`], as they appear in the TOML file.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub bind_addr: Option<IpAddr>,
    pub port: Option<u16>,
    pub cert_path: Option<PathBuf>,
    pub key_path: Option<PathBuf>,
    pub display: Option<u32>,
    pub max_fps: Option<u32>,
    pub log_level: Option<String>,
    pub encoder: Option<EncoderKind>,
}

impl FileConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading config file `{}`", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing config file `{}`", path.display()))
    }
}

/// Where the configuration came from, so that the startup log can say it
/// (F-112.7).
///
/// Kept apart from [`Config`] on purpose: the resolved values are what the
/// server runs on, the source is how it got them. Mixing the two is how a
/// surprising port turns into a debugging session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// A file named by `--config`. Reading it takes the default file out of
    /// play entirely: one file or the other, never both (F-112.2).
    Requested(PathBuf),
    /// The default file, which was there (F-112.1).
    Default(PathBuf),
    /// No file was read: there is none, or `--no-config` was given. A normal
    /// state, and therefore not a warning (F-112.4).
    Builtin,
}

impl ConfigSource {
    /// Decides which file applies, without reading its contents.
    ///
    /// `home` is a parameter rather than a read of `HOME` so that the decision
    /// is testable without touching the real home directory (F-112.9).
    fn decide(cli: &Cli, home: Option<&Path>) -> Self {
        if let Some(path) = cli.config.as_deref() {
            return Self::Requested(path.to_owned());
        }
        if cli.no_config {
            return Self::Builtin;
        }
        // F-112.3: no HOME means there is no default file. Not an error and
        // not a panic — every other way of configuring the server still works.
        let Some(home) = home else {
            return Self::Builtin;
        };
        let path = home.join(DEFAULT_CONFIG_FILE);
        match path.try_exists() {
            // F-112.4: the ordinary case, and silent.
            Ok(false) => Self::Builtin,
            // It is there, or we cannot even tell. Either way the user wrote
            // that file meaning it to be read, so read it and let any failure
            // be loud rather than working on defaults behind their back
            // (F-112.5).
            _ => Self::Default(path),
        }
    }

    /// The file to read, if any.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Requested(path) | Self::Default(path) => Some(path),
            Self::Builtin => None,
        }
    }
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Requested(path) | Self::Default(path) => write!(f, "{}", path.display()),
            Self::Builtin => f.write_str("built-in defaults"),
        }
    }
}

/// Fully resolved configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub bind_addr: IpAddr,
    pub port: u16,
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub display: u32,
    pub max_fps: u32,
    pub log_level: String,
    /// `Some((width, height))` when the synthetic pattern was requested.
    pub test_pattern: Option<(u32, u32)>,
    pub encoder: EncoderKind,
}

impl Config {
    /// Merges the three sources. Precedence: CLI > file > default (F-001.4).
    pub fn resolve(cli: &Cli, file: &FileConfig) -> Result<Self> {
        let default_dir = default_data_dir()?;

        Ok(Self {
            bind_addr: cli
                .bind_addr
                .or(file.bind_addr)
                .unwrap_or_else(|| DEFAULT_BIND_ADDR.parse().expect("valid hardcoded IP")),
            port: cli.port.or(file.port).unwrap_or(DEFAULT_PORT),
            cert_path: cli
                .cert_path
                .clone()
                .or_else(|| file.cert_path.clone())
                .unwrap_or_else(|| default_dir.join("server-cert.pem")),
            key_path: cli
                .key_path
                .clone()
                .or_else(|| file.key_path.clone())
                .unwrap_or_else(|| default_dir.join("server-key.pem")),
            display: cli.display.or(file.display).unwrap_or(DEFAULT_DISPLAY),
            max_fps: cli.max_fps.or(file.max_fps).unwrap_or(DEFAULT_MAX_FPS),
            log_level: cli
                .log_level
                .clone()
                .or_else(|| file.log_level.clone())
                .unwrap_or_else(|| DEFAULT_LOG_LEVEL.to_owned()),
            test_pattern: cli.test_pattern.as_deref().map(parse_size).transpose()?,
            encoder: cli.encoder.or(file.encoder).unwrap_or_default(),
        })
    }

    /// Loads whichever file applies and resolves everything (F-112.2).
    ///
    /// Returns the source next to the configuration because the caller has to
    /// log it: the log line is the whole of F-112.7.
    pub fn from_cli(cli: &Cli) -> Result<(Self, ConfigSource)> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Self::from_cli_in(cli, home.as_deref())
    }

    /// [`Self::from_cli`] with the home directory passed in, so that the whole
    /// path — not just the decision — is testable without the real `$HOME`
    /// (F-112.9).
    fn from_cli_in(cli: &Cli, home: Option<&Path>) -> Result<(Self, ConfigSource)> {
        let source = ConfigSource::decide(cli, home);
        let file = match source.path() {
            Some(path) => FileConfig::load(path)?,
            None => FileConfig::default(),
        };
        Ok((Self::resolve(cli, &file)?, source))
    }

    #[must_use]
    pub fn socket_addr(&self) -> std::net::SocketAddr {
        std::net::SocketAddr::new(self.bind_addr, self.port)
    }
}

/// Parses a `WIDTHxHEIGHT` string.
fn parse_size(text: &str) -> Result<(u32, u32)> {
    let (w, h) = text
        .split_once(['x', 'X'])
        .with_context(|| format!("`{text}` is not WIDTHxHEIGHT"))?;
    let width: u32 = w.trim().parse().with_context(|| format!("bad width in `{text}`"))?;
    let height: u32 = h.trim().parse().with_context(|| format!("bad height in `{text}`"))?;
    if width == 0 || height == 0 {
        anyhow::bail!("`{text}` has a zero dimension");
    }
    Ok((width, height))
}

/// `~/Library/Application Support/mac-rdp-server`, where the generated
/// certificate lives by default.
fn default_data_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set, cannot pick a default data directory")?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("mac-rdp-server"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_with_port(port: u16) -> FileConfig {
        FileConfig {
            port: Some(port),
            ..FileConfig::default()
        }
    }

    #[test]
    fn defaults_apply_when_nothing_is_given() {
        let cfg = Config::resolve(&Cli::default(), &FileConfig::default()).expect("resolve");
        assert_eq!(cfg.port, DEFAULT_PORT);
        assert_eq!(cfg.bind_addr.to_string(), DEFAULT_BIND_ADDR);
        assert_eq!(cfg.max_fps, DEFAULT_MAX_FPS);
        assert_eq!(cfg.display, DEFAULT_DISPLAY);
        assert_eq!(cfg.log_level, DEFAULT_LOG_LEVEL);
    }

    #[test]
    fn file_overrides_default() {
        let cfg = Config::resolve(&Cli::default(), &file_with_port(4000)).expect("resolve");
        assert_eq!(cfg.port, 4000);
    }

    #[test]
    fn cli_overrides_file() {
        let cli = Cli {
            port: Some(5000),
            ..Cli::default()
        };
        let cfg = Config::resolve(&cli, &file_with_port(4000)).expect("resolve");
        assert_eq!(cfg.port, 5000, "CLI flag must win over the file");
    }

    #[test]
    fn cli_overrides_file_for_every_key() {
        let cli = Cli {
            bind_addr: Some("0.0.0.0".parse().expect("valid IP")),
            port: Some(1),
            cert_path: Some(PathBuf::from("/cli/cert.pem")),
            key_path: Some(PathBuf::from("/cli/key.pem")),
            display: Some(2),
            max_fps: Some(3),
            log_level: Some("trace".to_owned()),
            config: None,
            no_config: false,
            test_pattern: None,
            encoder: Some(EncoderKind::Full),
            // Not a configuration key: `--check-permissions` is a mode that
            // exits before any config is resolved (F-110.15).
            check_permissions: false,
        };
        let file = FileConfig {
            bind_addr: Some("10.0.0.1".parse().expect("valid IP")),
            port: Some(9),
            cert_path: Some(PathBuf::from("/file/cert.pem")),
            key_path: Some(PathBuf::from("/file/key.pem")),
            display: Some(8),
            max_fps: Some(7),
            log_level: Some("error".to_owned()),
            encoder: Some(EncoderKind::Damage),
        };

        let cfg = Config::resolve(&cli, &file).expect("resolve");
        assert_eq!(cfg.bind_addr.to_string(), "0.0.0.0");
        assert_eq!(cfg.port, 1);
        assert_eq!(cfg.cert_path, PathBuf::from("/cli/cert.pem"));
        assert_eq!(cfg.key_path, PathBuf::from("/cli/key.pem"));
        assert_eq!(cfg.display, 2);
        assert_eq!(cfg.max_fps, 3);
        assert_eq!(cfg.log_level, "trace");
        assert_eq!(cfg.encoder, EncoderKind::Full);
    }

    #[test]
    fn toml_parses_the_documented_keys() {
        let file: FileConfig = toml::from_str(
            r#"
            bind_addr = "0.0.0.0"
            port = 3390
            cert_path = "/tmp/c.pem"
            key_path = "/tmp/k.pem"
            display = 1
            max_fps = 60
            log_level = "debug"
            "#,
        )
        .expect("parse");

        assert_eq!(file.port, Some(3390));
        assert_eq!(file.max_fps, Some(60));
        assert_eq!(file.log_level.as_deref(), Some("debug"));
    }

    #[test]
    fn unknown_toml_keys_are_an_error_not_a_silent_no_op() {
        let result = toml::from_str::<FileConfig>("prot = 3390\n");
        assert!(result.is_err(), "a typo in the config must be reported");
    }

    /// A throwaway home directory: the tag keeps the tests apart from each
    /// other, the process id keeps two runs apart.
    fn temp_home(tag: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!("mac-rdp-config-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".config")).expect("create a throwaway home");
        home
    }

    fn write_default_config(home: &Path, body: &str) -> PathBuf {
        let path = home.join(DEFAULT_CONFIG_FILE);
        std::fs::write(&path, body).expect("write the default config file");
        path
    }

    /// F-112.1, AC-1: the measurement from 2026-10-15 came out the other way
    /// round — the file was there and the server listened on the defaults.
    #[test]
    fn f112_1_the_default_file_is_read_without_any_flag() {
        let home = temp_home("default");
        let path = write_default_config(&home, "bind_addr = \"0.0.0.0\"\nport = 33891\n");

        let (cfg, source) = Config::from_cli_in(&Cli::default(), Some(&home)).expect("resolve");

        assert_eq!(cfg.socket_addr().to_string(), "0.0.0.0:33891");
        assert_eq!(source, ConfigSource::Default(path));
    }

    /// F-112.2, AC-2: a flag beats the default file, and beats only the key it
    /// names.
    #[test]
    fn f112_2_a_flag_wins_over_the_default_file() {
        let home = temp_home("flag");
        write_default_config(&home, "bind_addr = \"0.0.0.0\"\nport = 33891\n");
        let cli = Cli {
            bind_addr: Some("127.0.0.1".parse().expect("valid IP")),
            ..Cli::default()
        };

        let (cfg, _) = Config::from_cli_in(&cli, Some(&home)).expect("resolve");

        assert_eq!(cfg.bind_addr.to_string(), "127.0.0.1", "the flag wins");
        assert_eq!(cfg.port, 33891, "and leaves the rest of the file alone");
    }

    /// F-112.2, AC-3: one file or the other, never both. The default file
    /// carries a key the requested one does not, so reading both would show.
    #[test]
    fn f112_2_a_requested_file_replaces_the_default_file() {
        let home = temp_home("requested");
        write_default_config(&home, "bind_addr = \"0.0.0.0\"\nport = 33891\n");
        let requested = home.join("elsewhere.toml");
        std::fs::write(&requested, "port = 4242\n").expect("write the requested file");
        let cli = Cli {
            config: Some(requested.clone()),
            ..Cli::default()
        };

        let (cfg, source) = Config::from_cli_in(&cli, Some(&home)).expect("resolve");

        assert_eq!(cfg.port, 4242);
        assert_eq!(
            cfg.bind_addr.to_string(),
            DEFAULT_BIND_ADDR,
            "the default file must not be read at all, so its bind_addr cannot leak in"
        );
        assert_eq!(source, ConfigSource::Requested(requested));
    }

    /// F-112.4, AC-4: no file is the normal state, not a complaint.
    #[test]
    fn f112_4_no_default_file_means_built_in_defaults() {
        let home = temp_home("absent");

        let (cfg, source) = Config::from_cli_in(&Cli::default(), Some(&home)).expect("resolve");

        assert_eq!(cfg.port, DEFAULT_PORT);
        assert_eq!(source, ConfigSource::Builtin);
    }

    /// F-112.5, AC-5: a typo in the file the user wrote stops the server. Quiet
    /// work on the defaults would be worse — it hides the typo for as long as
    /// nobody notices the port.
    #[test]
    fn f112_5_an_unparsable_default_file_is_an_error() {
        let home = temp_home("broken");
        let path = write_default_config(&home, "bind_addr = \"not-an-address\"\n");

        let error = Config::from_cli_in(&Cli::default(), Some(&home)).expect_err("must not start");

        let message = format!("{error:#}");
        assert!(
            message.contains(&path.display().to_string()),
            "the message must name the file: {message}"
        );
        assert!(
            message.contains("parsing config file"),
            "and say what went wrong with it: {message}"
        );
    }

    /// F-112.3: without `HOME` there is simply no default file.
    #[test]
    fn f112_3_without_home_there_is_no_default_file() {
        assert_eq!(ConfigSource::decide(&Cli::default(), None), ConfigSource::Builtin);
    }

    /// F-112.6: the escape hatch, tested on exactly the file that would
    /// otherwise stop the server — that is the situation it exists for.
    #[test]
    fn f112_6_no_config_skips_the_default_file() {
        let home = temp_home("skip");
        write_default_config(&home, "bind_addr = \"not-an-address\"\n");
        let cli = Cli {
            no_config: true,
            ..Cli::default()
        };

        let (cfg, source) = Config::from_cli_in(&cli, Some(&home)).expect("must start anyway");

        assert_eq!(cfg.port, DEFAULT_PORT);
        assert_eq!(source, ConfigSource::Builtin);
    }

    /// F-112.6: the two flags answer the same question, so asking both ways at
    /// once is a usage error rather than a silent precedence rule.
    #[test]
    fn f112_6_no_config_and_config_together_are_rejected() {
        let result = Cli::try_parse_from(["mac-rdp-server", "--no-config", "--config", "some.toml"]);
        assert!(result.is_err(), "--no-config with --config must not be accepted");
    }

    #[test]
    fn socket_addr_combines_host_and_port() {
        let cli = Cli {
            bind_addr: Some("192.168.1.5".parse().expect("valid IP")),
            port: Some(3389),
            ..Cli::default()
        };
        let cfg = Config::resolve(&cli, &FileConfig::default()).expect("resolve");
        assert_eq!(cfg.socket_addr().to_string(), "192.168.1.5:3389");
    }
}
