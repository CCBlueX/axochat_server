use crate::error::*;
use log::*;

use crate::chat::{ChatServer, ClientPacket, InternalId, SuccessReason, User, UserSession, send_message};
use crate::message::RateLimiter;
use std::collections::HashSet;

use crate::auth::authenticate;
use actix::*;
use rand::Rng;
use std::str::FromStr;
use uuid::Uuid;

impl ChatServer {
    pub(super) fn handle_request_mojang_info(&mut self, user_id: InternalId) {
        let Some(session) = self.connections.get_mut(&user_id) else { return };

        let mut bytes = [0; 20];
        self.rng.fill_bytes(&mut bytes);
        // we'll just ignore one bit so we that don't have to deal with a '-' sign
        bytes[0] &= 0b0111_1111;

        let session_hash = crate::auth::encode_sha1_bytes(&bytes);
        session.session_hash = Some(session_hash.clone());

        send_message(session, ClientPacket::MojangInfo { session_hash }, "mojang info");
    }

    pub(super) fn login_mojang(
        &mut self,
        user_id: InternalId,
        info: User,
        ctx: &mut Context<Self>,
    ) {
        let Some(session) = self.connections.get_mut(&user_id) else { return };

        if session.is_logged_in() || session.login_pending {
            info!("User `{}` tried to log in multiple times.", user_id);
            send_message(session, ClientPacket::error(ClientError::AlreadyLoggedIn), "mojang already logged in");
            return;
        }

        let Some(session_hash) = session.session_hash.clone() else {
            info!(
                "User `{}` did not request mojang info, but tried to log in.",
                user_id
            );
            send_message(session, ClientPacket::error(ClientError::MojangRequestMissing), "mojang info missing");
            return;
        };

        session.login_pending = true;
        let name = info.name.clone();

        ctx.spawn(
            async move { authenticate(&name, &session_hash).await }
                .into_actor(self)
                .map(move |res, actor, _ctx| {
                    let Some(session) = actor.connections.get_mut(&user_id) else { return };
                    session.login_pending = false;

                    let mojang_info = match res {
                        Ok(mojang_info) => mojang_info,
                        Err(err) => {
                            warn!("Could not authenticate user `{}`: {}", user_id, err);
                            send_message(session, ClientPacket::error(ClientError::LoginFailed), "mojang login failed");
                            return;
                        }
                    };

                    if Uuid::from_str(&mojang_info.id).ok() != Some(info.uuid) {
                        send_message(session, ClientPacket::error(ClientError::InvalidId), "mojang invalid id");
                        return;
                    }

                    info!(
                        "User `{}` has uuid `{}` and username `{}`",
                        user_id, mojang_info.id, mojang_info.name
                    );

                    actor
                        .users
                        .entry(mojang_info.name.clone())
                        .or_insert_with(|| UserSession {
                            rate_limiter: RateLimiter::new(actor.config.message.clone()),
                            connections: HashSet::new(),
                        })
                        .connections
                        .insert(user_id);

                    session.user = Some(User {
                        name: mojang_info.name,
                        ..info
                    });

                    send_message(session, ClientPacket::Success {
                        reason: SuccessReason::Login,
                    }, "mojang login success");
                }),
        );
    }
}
