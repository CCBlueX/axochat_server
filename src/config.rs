use crate::error::*;
use jsonwebtoken::Algorithm;
use std::{env, fmt, net::SocketAddr, path::PathBuf, str::FromStr, time::Duration};

#[derive(Clone)]
pub struct Config {
    pub net: NetConfig,
    pub message: MsgConfig,
    pub moderation: ModConfig,
    pub auth: Option<AuthConfig>,
}

#[derive(Clone)]
pub struct NetConfig {
    /// The address the server will listen at.
    pub address: SocketAddr,

    /// The SSL certificate file.
    pub cert_file: Option<PathBuf>,
    /// The SSL key file.
    /// If the extension is `pem`, `PEM` format will be used, otherwise `ASN1`.
    pub key_file: Option<PathBuf>,
}

#[derive(Clone)]
pub struct MsgConfig {
    /// The maximum message length in chars.
    pub max_length: usize,

    /// The maximum amount of messages in `count_duration`.
    pub max_messages: usize,

    /// The duration in which the amount of messages cannot be greater.
    pub count_duration: Duration,
}

#[derive(Clone)]
pub struct AuthConfig {
    /// The key of the JWT
    pub secret: String,

    /// The JWT algorithm
    pub algorithm: Algorithm,

    /// The time for which a JWT is valid
    pub valid_time: Duration,
}

#[derive(Clone)]
pub struct ModConfig {
    /// The file containing the moderators (line separated).
    pub moderators: PathBuf,

    /// The file containing the banned users (line separated).
    pub banned: PathBuf,
}

fn env_opt(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.is_empty())
}

fn env_parse<T>(key: &'static str) -> Result<Option<T>>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    env_opt(key)
        .map(|value| {
            value.parse().map_err(|err: T::Err| Error::InvalidEnv {
                key,
                reason: err.to_string(),
            })
        })
        .transpose()
}

impl Config {
    pub fn from_env() -> Result<Config> {
        Ok(Config {
            net: NetConfig {
                address: env_parse("SERVER_ADDR")?.unwrap_or_else(|| ([0, 0, 0, 0], 8080).into()),
                cert_file: env_parse("TLS_CERT_FILE")?,
                key_file: env_parse("TLS_KEY_FILE")?,
            },
            message: MsgConfig {
                max_length: env_parse("MESSAGE_MAX_LENGTH")?.unwrap_or(100),
                max_messages: env_parse("MESSAGE_MAX_MESSAGES")?.unwrap_or(40),
                count_duration: env_parse::<humantime::Duration>("MESSAGE_COUNT_DURATION")?
                    .map_or(Duration::from_secs(60), Into::into),
            },
            moderation: ModConfig {
                moderators: env_parse("MODERATORS_FILE")?
                    .unwrap_or_else(|| PathBuf::from("./moderators.txt")),
                banned: env_parse("BANNED_FILE")?.unwrap_or_else(|| PathBuf::from("./banned.txt")),
            },
            auth: match env_opt("JWT_SECRET") {
                Some(secret) => Some(AuthConfig {
                    secret,
                    algorithm: env_parse("JWT_ALGORITHM")?.unwrap_or(Algorithm::HS256),
                    valid_time: env_parse::<humantime::Duration>("JWT_VALID_TIME")?
                        .ok_or(Error::MissingEnv {
                            key: "JWT_VALID_TIME",
                        })?
                        .into(),
                }),
                None => None,
            },
        })
    }
}
