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
        if connection.login == Login::Anonymous && connection.session_hash.is_none() {
            info!("User `{}` did not request mojang info, but tried to log in.", user_id);
            self.send_error(user_id, ClientError::MojangRequestMissing);
            return;
        }
        let session_hash = connection.session_hash.clone().unwrap_or_default();
        let Some(visit) = self.begin_login(user_id) else { return };

        let session_url = self.config.mojang.session_url.clone();
        let (api, store, logins) = (self.api.clone(), self.store.clone(), self.logins.clone());

        ctx.spawn(
            async move {
                let _permit = logins.acquire().await;
                let profile = authenticate(&session_url, &info.name, &session_hash).await.map_err(|err| {
                    warn!("Could not authenticate user `{}`: {}", user_id, err);
                    ClientError::LoginFailed
                })?;
                if Uuid::from_str(&profile.id).ok() != Some(info.uuid) {
                    return Err(ClientError::InvalidId);
                }

                // a Minecraft account linked to a LiquidBounce Account logs in as that account
                let linked = api.linked_account(info.uuid).await.unwrap_or_else(|err| {
                    warn!("Could not look up the account linked to `{}`: {}", info.uuid, err);
                    None
                });
                let (request, roles) = match linked {
                    Some(account) => (
                        visit.identify(
                            IdentityKey::Account(account.user_id),
                            account.nickname.unwrap_or(profile.name),
                            Some(info.uuid),
                        ),
                        account.roles,
                    ),
                    None => (visit.identify(IdentityKey::Minecraft(info.uuid), profile.name, None), Vec::new()),
                };
                let model = identify(store, request).await?;
                Ok(Verified { model, roles })
            }
            .into_actor(self)
            .map(move |result, actor, _ctx| actor.finish_login(user_id, result, info.allow_messages)),
        );
    }
}
