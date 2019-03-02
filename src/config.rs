use crate::error::*;
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, File},
    io::{self, Read},
    net::SocketAddr,
    path::PathBuf,
};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub net: NetConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetConfig {
    pub address: SocketAddr,
}

impl Default for NetConfig {
    fn default() -> NetConfig {
        NetConfig {
            address: ([127, 0, 0, 1], 8080).into(),
        }
    }
}

/// Reads the configuration file at `$CONFIG_PATH` or creates one if none was found.
pub fn read_config() -> Result<Config> {
    let path = env::var("CONFIG_PATH").unwrap_or_else(|_| String::from("./ferrmontis.toml"));
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
