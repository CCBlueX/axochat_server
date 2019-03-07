use crate::error::*;
use jsonwebtoken::Algorithm;
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, File},
    io::{self, Read},
    net::SocketAddr,
    path::PathBuf,
    time::Duration,
};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub net: NetConfig,

    #[serde(default)]
    pub message: MsgConfig,

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
    /// The maximum message length in bytes.
    pub max_length: usize,

    /// The maximum amount of messages in `count_duration`.
    pub max_messages: usize,

    /// The duration in which the amount of messages cannot be greater.
    pub count_duration: Duration,
}

impl Default for MsgConfig {
    fn default() -> MsgConfig {
        MsgConfig {
            max_length: 100,
            max_messages: 40,
            count_duration: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthConfig {
    /// The file containing the key of the JWT
    pub key_file: PathBuf,

    /// The JWT algorithm
    pub algorithm: Algorithm,
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
