use super::login::{identify, Verified};
use crate::chat::world::Player;
use crate::chat::{ChatServer, ClientPacket, InternalId, Kind, Login, Protocol, SuccessReason, User, UserId};
use crate::error::ClientError;
use crate::store::IdentityKey;
use log::*;

use crate::auth::{authenticate, AuthInfo};
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
        if let Login::User(user) = connection.login {
            if connection.protocol >= Protocol::V2 && self.identity(user).kind == Kind::Account {
                self.prove_minecraft(user_id, user, info, ctx);
                return;
            }
        }
        if connection.login == Login::Anonymous && connection.session_hash.is_none() {
            info!("User `{}` did not request mojang info, but tried to log in.", user_id);
            self.send_error(user_id, ClientError::MojangRequestMissing);
            return;
        }
        let session_hash = connection.session_hash.clone().unwrap_or_default();
        let Some(visit) = self.begin_login(user_id) else { return };

        let allow_messages = info.allow_messages;
        let session_url = self.config.mojang.session_url.clone();
        let (api, store, logins) = (self.api.clone(), self.store.clone(), self.logins.clone());

        ctx.spawn(
            async move {
                let _permit = logins.acquire().await;
                let profile = verify(&session_url, user_id, &info, &session_hash).await?;

                // a Minecraft account linked to a LiquidBounce Account logs in as that account
                let linked = api.linked_account(info.uuid).await.unwrap_or_else(|err| {
                    warn!("Could not look up the account linked to `{}`: {}", info.uuid, err);
                    None
                });
                let (request, roles, minecraft) = match linked {
                    Some(account) => (
                        visit.identify(
                            IdentityKey::Account(account.user_id),
                            account.nickname.unwrap_or_else(|| profile.name.clone()),
                            Some(info.uuid),
                        ),
                        account.roles,
                        Some(Player {
                            uuid: info.uuid,
                            name: profile.name,
                        }),
                    ),
                    None => (visit.identify(IdentityKey::Minecraft(info.uuid), profile.name, None), Vec::new(), None),
                };
                let model = identify(store, request).await?;
                Ok(Verified { model, roles, minecraft })
            }
            .into_actor(self)
            .map(move |result, actor, _ctx| actor.finish_login(user_id, result, allow_messages)),
        );
    }

    fn prove_minecraft(&mut self, user_id: InternalId, user: UserId, info: User, ctx: &mut Context<Self>) {
        let Some(session_hash) = self.connections.get_mut(&user_id).and_then(|connection| connection.session_hash.take())
        else {
            self.send_error(user_id, ClientError::MojangRequestMissing);
            return;
        };
        let session_url = self.config.mojang.session_url.clone();
        let logins = self.logins.clone();

        ctx.spawn(
            async move {
                let _permit = logins.acquire().await;
                let profile = verify(&session_url, user_id, &info, &session_hash).await?;
                Ok(Player {
                    uuid: info.uuid,
                    name: profile.name,
                })
            }
            .into_actor(self)
            .map(move |result: Result<Player, ClientError>, actor, _ctx| {
                let Some(connection) = actor.connections.get_mut(&user_id) else { return };
                if connection.login != Login::User(user) {
                    return;
                }
                match result {
                    Ok(player) => {
                        connection.minecraft = Some(player);
                        actor.send(user_id, ClientPacket::Success { reason: SuccessReason::Minecraft });
                        for (friend, _) in actor.social.friends(user) {
                            actor.send_friends(friend);
                        }
                        actor.refresh_party(user);
                    }
                    Err(error) => actor.send_error(user_id, error),
                }
            }),
        );
    }
}

async fn verify(session_url: &str, user_id: InternalId, info: &User, session_hash: &str) -> Result<AuthInfo, ClientError> {
    let profile = authenticate(session_url, &info.name, session_hash).await.map_err(|err| {
        warn!("Could not authenticate user `{}`: {}", user_id, err);
        ClientError::LoginFailed
    })?;
    if Uuid::from_str(&profile.id).ok() != Some(info.uuid) {
        return Err(ClientError::InvalidId);
    }
    Ok(profile)
}
