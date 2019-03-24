use crate::error::*;
use log::*;

use actix_web::{client::ClientRequest, http::StatusCode, HttpMessage};
use futures::Future;
use serde::{de::IgnoredAny, Deserialize, Serialize};
use url::Url;

use crate::config::AuthConfig;
use jsonwebtoken::{Header, Validation};
use uuid::Uuid;
use std::{
    fs,
    time::{Duration, SystemTime},
};

pub fn authenticate(
    username: &str,
    server_id: &str,
) -> Result<impl Future<Item = AuthInfo, Error = Error>> {
    let mut url =
        Url::parse("https://sessionserver.mojang.com/session/minecraft/hasJoined").unwrap();
    url.query_pairs_mut()
        .append_pair("username", username)
        .append_pair("serverId", server_id);

    Ok(ClientRequest::get(url)
        .finish()?
        .send()
        .map_err(|err| Error::Actix(err.into()))
        .and_then(|response| {
            if response.status() == StatusCode::OK {
                Ok(response)
            } else {
                debug!("Login status-code is {}", response.status());
                Err(ClientError::LoginFailed.into())
            }
        })
        .and_then(|response| response.json().map_err(|err| Error::Actix(err.into()))))
}

#[derive(Debug, Deserialize)]
pub struct AuthInfo {
    pub id: String,
    pub name: String,
    properties: IgnoredAny,
}

pub fn encode_sha1_bytes(bytes: &[u8; 20]) -> String {
    const HEX_ALPHABET: [char; 16] = [
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
    ];

    let mut buf = String::with_capacity(40);
    let mut skipped_zeros = false;
    for &byte in bytes.iter() {
        let left = byte >> 4;
        if left != 0 {
            skipped_zeros = true;
        }
        if skipped_zeros {
            buf.push(HEX_ALPHABET[left as usize]);
        }

        let right = byte & 0b1111;
        if right != 0 {
            skipped_zeros = true;
        }
        if skipped_zeros {
            buf.push(HEX_ALPHABET[right as usize]);
        }
    }

    if buf.is_empty() {
        buf.push(HEX_ALPHABET[0]);
    }

    buf
}

pub struct Authenticator {
    validation: Validation,
    header: Header,
    key: Vec<u8>,
    valid_time: Duration,
}

impl Authenticator {
    pub fn new(cfg: &AuthConfig) -> Result<Authenticator> {
        Ok(Authenticator {
            validation: Validation::new(cfg.algorithm),
            header: Header::new(cfg.algorithm),
            key: fs::read(&cfg.key_file)?,
            valid_time: cfg.valid_time,
        })
    }

    pub fn auth(&self, token: &str) -> Result<UserInfo> {
        match jsonwebtoken::decode(token, &self.key, &self.validation) {
            Ok(data) => Ok(data.claims),
            Err(err) => Err(err.into()),
        }
    }

    pub fn new_token(&self, info: UserInfo) -> Result<String> {
        let unix_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("system time is somehow before the unix epoch");
        let claims = Claims {
            exp: (unix_time + self.valid_time).as_secs(),
            user: info,
        };
        jsonwebtoken::encode(&self.header, &claims, &self.key).map_err(|err| err.into())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    exp: u64,
    user: UserInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub name: String,
    pub uuid: Uuid,
}
