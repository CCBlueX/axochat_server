use crate::ip::TrustedProxy;
use clap::Args;
use jsonwebtoken::Algorithm;
use std::{net::SocketAddr, path::PathBuf, time::Duration};

#[derive(Args, Clone)]
pub struct Config {
    #[command(flatten)]
    pub net: NetConfig,

    #[command(flatten)]
    pub message: MsgConfig,

    #[command(flatten)]
    pub moderation: ModConfig,

    #[command(flatten)]
    pub auth: AuthConfig,
}

#[derive(Args, Clone)]
pub struct NetConfig {
    /// The address the server will listen at.
    #[arg(long = "server-addr", env = "SERVER_ADDR", default_value = "0.0.0.0:8080")]
    pub address: SocketAddr,

    /// The SSL certificate file.
    #[arg(long = "tls-cert-file", env = "TLS_CERT_FILE")]
    pub cert_file: Option<PathBuf>,
    /// The SSL key file.
    /// If the extension is `pem`, `PEM` format will be used, otherwise `ASN1`.
    #[arg(long = "tls-key-file", env = "TLS_KEY_FILE")]
    pub key_file: Option<PathBuf>,

    /// Proxies whose client address header is believed: CIDRs, or `cloudflare` for its ranges.
    #[arg(long = "trusted-proxies", env = "TRUSTED_PROXIES", value_delimiter = ',')]
    pub trusted_proxies: Vec<TrustedProxy>,

    /// The header a trusted proxy puts the client address in.
    #[arg(long = "real-ip-header", env = "REAL_IP_HEADER", default_value = "CF-Connecting-IP")]
    pub real_ip_header: String,
}

#[derive(Args, Clone)]
pub struct MsgConfig {
    /// The maximum message length in chars.
    #[arg(long = "message-max-length", env = "MESSAGE_MAX_LENGTH", default_value_t = 100)]
    pub max_length: usize,

    /// The maximum amount of messages in `count_duration`.
    #[arg(long = "message-max-messages", env = "MESSAGE_MAX_MESSAGES", default_value_t = 40)]
    pub max_messages: usize,

    /// The duration in which the amount of messages cannot be greater.
    #[arg(
        long = "message-count-duration",
        env = "MESSAGE_COUNT_DURATION",
        default_value = "1m",
        value_parser = humantime::parse_duration
    )]
    pub count_duration: Duration,
}

#[derive(Args, Clone)]
pub struct AuthConfig {
    /// The key of the JWT. JWT login is disabled without it.
    #[arg(
        long = "jwt-secret",
        env = "JWT_SECRET",
        hide_env_values = true,
        requires = "valid_time"
    )]
    pub secret: Option<String>,

    /// The JWT algorithm
    #[arg(long = "jwt-algorithm", env = "JWT_ALGORITHM", default_value = "HS256")]
    pub algorithm: Algorithm,

    /// The time for which a JWT is valid
    #[arg(long = "jwt-valid-time", env = "JWT_VALID_TIME", value_parser = humantime::parse_duration)]
    pub valid_time: Option<Duration>,
}

#[derive(Args, Clone)]
pub struct ModConfig {
    /// The file containing the moderators (line separated).
    #[arg(long = "moderators-file", env = "MODERATORS_FILE", default_value = "./moderators.txt")]
    pub moderators: PathBuf,

    /// The file containing the banned users (line separated).
    #[arg(long = "banned-file", env = "BANNED_FILE", default_value = "./banned.txt")]
    pub banned: PathBuf,
}
