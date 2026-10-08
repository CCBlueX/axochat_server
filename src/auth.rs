use crate::error::*;
use log::*;

use reqwest::{self, StatusCode};
use serde::{de::IgnoredAny, Deserialize};
use url::Url;

use std::{sync::OnceLock, time::Duration};

pub async fn authenticate(session_url: &str, username: &str, server_id: &str) -> Result<AuthInfo> {
    let mut url = Url::parse(&format!("{}/session/minecraft/hasJoined", session_url.trim_end_matches('/')))
        .map_err(|err| Error::IO { source: std::io::Error::new(std::io::ErrorKind::InvalidInput, err) })?;
    url.query_pairs_mut()
        .append_pair("username", username)
        .append_pair("serverId", server_id);

    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("could not build http client")
    });
    let response = client
        .get(url.as_str())
        .send()
        .await
        .map_err(|err| {
            debug!("Reqwest error: {:?}", err);
            Error::IO { source: std::io::Error::new(std::io::ErrorKind::Other, err) }
        })?;

    if response.status() == StatusCode::OK {
        response
            .json::<AuthInfo>()
            .await
            .map_err(|err| {
                debug!("JSON deserialization error: {:?}", err);
                Error::IO { source: std::io::Error::new(std::io::ErrorKind::Other, err) }
            })
    } else {
        debug!("Login status-code is {}", response.status());
        Err(ClientError::LoginFailed.into())
    }
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

#[cfg(test)]
mod tests {
    use super::encode_sha1_bytes;

    #[test]
    fn session_hash_drops_leading_zeros() {
        let mut bytes = [0u8; 20];
        assert_eq!(encode_sha1_bytes(&bytes), "0");
        bytes[19] = 0x0f;
        assert_eq!(encode_sha1_bytes(&bytes), "f");
        bytes[0] = 0x01;
        assert_eq!(encode_sha1_bytes(&bytes), "10000000000000000000000000000000000000f");
    }
}
