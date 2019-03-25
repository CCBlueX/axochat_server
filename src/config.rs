use crate::error::*;
use jsonwebtoken::Algorithm;
use serde::{
    de::{self, Deserializer, Visitor},
    ser::Serializer,
    Deserialize, Serialize,
};
use std::{
    env,
    fs::{self, File},
    io::{self, Read},
    net::SocketAddr,
    path::PathBuf,
    time::Duration,
    ops::Deref,
    fmt,
};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub net: NetConfig,

    #[serde(default)]
    pub message: MsgConfig,

    #[serde(default)]
    pub moderation: ModConfig,

    pub auth: Option<AuthConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NetConfig {
    /// The address the server will listen at.
    pub address: SocketAddr,

    /// The SSL certificate file.
    pub cert_file: Option<PathBuf>,
    /// The SSL key file.
    /// If the extension is `pem`, `PEM` format will be used, otherwise `ASN1`.
    pub key_file: Option<PathBuf>,
}

impl Default for NetConfig {
    fn default() -> NetConfig {
        NetConfig {
            address: ([127, 0, 0, 1], 8080).into(),
            cert_file: None,
            key_file: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MsgConfig {
    /// The maximum message length in chars.
    pub max_length: usize,

    /// The maximum amount of messages in `count_duration`.
    pub max_messages: usize,

    /// The duration in which the amount of messages cannot be greater.
    pub count_duration: WDuration,
}

impl Default for MsgConfig {
    fn default() -> MsgConfig {
        MsgConfig {
            max_length: 100,
            max_messages: 40,
            count_duration: Duration::from_secs(60).into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthConfig {
    /// The file containing the key of the JWT
    pub key_file: PathBuf,

    /// The JWT algorithm
    pub algorithm: Algorithm,

    /// The time for which a JWT is valid
    pub valid_time: WDuration,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModConfig {
    /// The file containing the moderators (line separated).
    pub moderators: PathBuf,

    /// The file containing the banned users (line separated).
    pub banned: PathBuf,
}

impl Default for ModConfig {
    fn default() -> ModConfig {
        ModConfig {
            moderators: PathBuf::from("./moderators.txt"),
            banned: PathBuf::from("./banned.txt"),
        }
    }
}

/// Reads the configuration file at `$CONFIG_PATH` or creates one if none was found.
pub fn read_config() -> Result<Config> {
    let path = env::var("CONFIG_PATH").unwrap_or_else(|_| String::from("./axochat.toml"));
    let path = PathBuf::from(path);

    match File::open(&path) {
        Ok(mut file) => {
            let mut input = String::new();
            file.read_to_string(&mut input)?;
            Ok(toml::from_str(&input)?)
        }
        Err(ref err) if err.kind() == io::ErrorKind::NotFound => {
            let cfg = Config::default();
            let output = toml::to_string_pretty(&cfg).unwrap();
            fs::write(path, output)?;
            Ok(cfg)
        }
        Err(err) => Err(err.into()),
    }
}

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub struct WDuration(Duration);

impl From<Duration> for WDuration {
    fn from(duration: Duration) -> WDuration {
        WDuration(duration)
    }
}

impl Deref for WDuration {
    type Target = Duration;

    fn deref(&self) -> &Duration {
        &self.0
    }
}

impl<'de> Deserialize<'de> for WDuration {
    fn deserialize<D>(deserializer: D) -> std::result::Result<WDuration, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct IdVisitor;

        impl<'de> Visitor<'de> for IdVisitor {
            type Value = WDuration;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a duration")
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                match humantime::parse_duration(value) {
                    Ok(duration) => Ok(WDuration(duration)),
                    Err(err) => Err(E::custom(err)),
                }
            }
        }

        deserializer.deserialize_str(IdVisitor)
    }
}

impl Serialize for WDuration {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let duration = humantime::format_duration(self.0);
        serializer.serialize_str(&duration.to_string())
    }
}
