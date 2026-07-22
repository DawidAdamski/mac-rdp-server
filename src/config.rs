//! Configuration: CLI flags over TOML file over defaults (F-001.4).

use std::net::IpAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use clap::Parser;
use serde::Deserialize;

pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 3389;
pub const DEFAULT_DISPLAY: u32 = 0;
pub const DEFAULT_MAX_FPS: u32 = 30;
pub const DEFAULT_LOG_LEVEL: &str = "info";

// Command line interface. Not a doc comment: clap would put it in `--help`.
//
// Every option is `Option<_>` so that "not given on the command line" is
// distinguishable from "given, and happens to equal the default" — otherwise
// flags could not take precedence over the file.
#[derive(Debug, Parser, Default)]
#[command(name = "mac-rdp-server", version, about = "RDP server for macOS")]
pub struct Cli {
    /// Path to a TOML configuration file.
    #[arg(short, long, value_name = "PATH")]
    pub config: Option<PathBuf>,

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
}

impl FileConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading config file `{}`", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing config file `{}`", path.display()))
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
        })
    }

    /// Loads the file named by `--config`, if any, and resolves everything.
    pub fn from_cli(cli: &Cli) -> Result<Self> {
        let file = match cli.config.as_deref() {
            Some(path) => FileConfig::load(path)?,
            None => FileConfig::default(),
        };
        Self::resolve(cli, &file)
    }

    #[must_use]
    pub fn socket_addr(&self) -> std::net::SocketAddr {
        std::net::SocketAddr::new(self.bind_addr, self.port)
    }
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
        };
        let file = FileConfig {
            bind_addr: Some("10.0.0.1".parse().expect("valid IP")),
            port: Some(9),
            cert_path: Some(PathBuf::from("/file/cert.pem")),
            key_path: Some(PathBuf::from("/file/key.pem")),
            display: Some(8),
            max_fps: Some(7),
            log_level: Some("error".to_owned()),
        };

        let cfg = Config::resolve(&cli, &file).expect("resolve");
        assert_eq!(cfg.bind_addr.to_string(), "0.0.0.0");
        assert_eq!(cfg.port, 1);
        assert_eq!(cfg.cert_path, PathBuf::from("/cli/cert.pem"));
        assert_eq!(cfg.key_path, PathBuf::from("/cli/key.pem"));
        assert_eq!(cfg.display, 2);
        assert_eq!(cfg.max_fps, 3);
        assert_eq!(cfg.log_level, "trace");
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
