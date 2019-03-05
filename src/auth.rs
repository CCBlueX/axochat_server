use crate::error::*;
use log::*;

use actix_web::{client::ClientRequest, http::StatusCode, HttpMessage};
use futures::Future;
use serde::{de::IgnoredAny, Deserialize};
use url::Url;

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
                Err(Error::LoginFailed)
            }
        })
        .and_then(|response| response.json().map_err(|err| Error::Actix(err.into()))))
}

#[derive(Debug, Deserialize)]
pub struct AuthInfo {
    id: String,
    name: String,
    properties: IgnoredAny,
}
