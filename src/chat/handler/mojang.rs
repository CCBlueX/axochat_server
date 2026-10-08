use super::login::{identify, Verified};
use crate::chat::{ChatServer, ClientPacket, InternalId, Login, User};
use crate::error::*;
use crate::store::IdentityKey;
use log::*;

use crate::auth::authenticate;
use actix::*;
use rand::Rng;
use std::str::FromStr;
use uuid::Uuid;

impl ChatServer {
    pub(super) fn handle_request_mojang_info(&mut self, user_id: InternalId) {
        let Some(connection) = self.connections.get_mut(&user_id) else { return };

        let mut bytes = [0; 20];
        self.rng.fill_bytes(&mut bytes);
        // we'll just ignore one bit so we that don't have to deal with a '-' sign
        bytes[0] &= 0b0111_1111;

        let session_hash = crate::auth::encode_sha1_bytes(&bytes);
        connection.session_hash = Some(session_hash.clone());
        self.send(user_id, ClientPacket::MojangInfo { session_hash });
    }

    pub(super) fn login_mojang(&mut self, user_id: InternalId, info: User, ctx: &mut Context<Self>) {
        let Some(connection) = self.connections.get(&user_id) else { return };
        let Some(session_hash) = connection.session_hash.clone().filter(|_| connection.login == Login::Anonymous) else {
            if connection.login == Login::Anonymous {
                info!("User `{}` did not request mojang info, but tried to log in.", user_id);
                self.send_error(user_id, ClientError::MojangRequestMissing);
            } else {
                self.send_error(user_id, ClientError::AlreadyLoggedIn);
            }
            return;
        };
        if !self.begin_login(user_id) {
            return;
        }

        let store = self.store.clone();
        let session_url = self.config.mojang.session_url.clone();
        let Some(mut request) = self.identify_request(user_id, IdentityKey::Minecraft(info.uuid), info.name.clone(), None) else {
            return;
        };

        ctx.spawn(
            async move {
                let profile = authenticate(&session_url, &info.name, &session_hash).await.map_err(|err| {
                    warn!("Could not authenticate user `{}`: {}", user_id, err);
                    ClientError::LoginFailed
                })?;
                if Uuid::from_str(&profile.id).ok() != Some(info.uuid) {
                    return Err(ClientError::InvalidId);
                }

                request.name = profile.name;
                let model = identify(store, request).await?;
                Ok(Verified { model })
            }
            .into_actor(self)
            .map(move |result, actor, _ctx| actor.finish_login(user_id, result, info.allow_messages)),
        );
    }
}
