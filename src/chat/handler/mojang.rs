use crate::error::*;
use log::*;

use crate::chat::{ChatServer, ClientPacket, InternalId, SuccessReason, User, UserSession, send_message};
use crate::message::RateLimiter;
use std::collections::HashSet;

use crate::auth::authenticate;
use actix::*;
use rand::RngCore;
use std::str::FromStr;
use uuid::Uuid;

impl ChatServer {
    pub(super) fn handle_request_mojang_info(&mut self, user_id: InternalId) {
        let session = self
            .connections
            .get_mut(&user_id)
            .expect("could not find connection");

        let mut bytes = [0; 20];
        self.rng.fill_bytes(&mut bytes);
        // we'll just ignore one bit so we that don't have to deal with a '-' sign
        bytes[0] &= 0b0111_1111;

        let session_hash = crate::auth::encode_sha1_bytes(&bytes);
        session.session_hash = Some(session_hash.clone());

        send_message(
            &session.addr,
            ClientPacket::MojangInfo { session_hash },
            "mojang info"
        );
    }

    pub(super) fn login_mojang(
        &mut self,
        user_id: InternalId,
        info: User,
        ctx: &mut Context<Self>,
    ) {
        let session = self
            .connections
            .get(&user_id)
            .expect("could not find connection");

        if session.is_logged_in() {
            info!("User `{}` tried to log in multiple times.", user_id);
            send_message(
                &session.addr,
                ClientPacket::Error {
                    message: ClientError::AlreadyLoggedIn,
                },
                "mojang already logged in"
            );
            return;
        }

        if let Some(session_hash) = &session.session_hash {
            // Clone the session_hash and user_id before moving them into the async block
            let session_hash = session_hash.clone();
            let name = info.name.clone();
            let uuid = info.uuid;
            let userid_for_closure = user_id;
            let session_addr = session.addr.clone();

            // Spawn a Future that performs the authentication
            ctx.spawn(
                async move {
                    authenticate(&name, &session_hash).await
                }
                .into_actor(self)
                .then(move |res, actor, _ctx| {
                    match res {
                        Ok(mojang_info)
                            if Uuid::from_str(&mojang_info.id)
                                .expect("got invalid uuid from mojang :()")
                                == uuid =>
                        {
                            info!(
                                "User `{}` has uuid `{}` and username `{}`",
                                userid_for_closure, mojang_info.id, mojang_info.name
                            );

                            if let Some(session) = actor.connections.get_mut(&userid_for_closure) {
                                actor
                                    .users
                                    .entry(info.name.clone())
                                    .or_insert(UserSession {
                                        rate_limiter: RateLimiter::new(
                                            actor.config.message.clone(),
                                        ),
                                        connections: HashSet::new(),
                                    })
                                    .connections
                                    .insert(userid_for_closure);

                                session.user = Some(info);

                                // Use standalone send_message function
                                send_message(
                                    &session.addr,
                                    ClientPacket::Success {
                                        reason: SuccessReason::Login,
                                    },
                                    "mojang login success"
                                );
                            }
                        }
                        Ok(_) => {
                            send_message(
                                &session_addr,
                                ClientPacket::Error {
                                    message: ClientError::InvalidId,
                                },
                                "mojang invalid id"
                            );
                        }
                        Err(err) => {
                            warn!("Could not authenticate user `{}`: {}", userid_for_closure, err);
                            send_message(
                                &session_addr,
                                ClientPacket::Error {
                                    message: ClientError::LoginFailed,
                                },
                                "mojang login failed"
                            );
                        }
                    }
                    fut::ready(())
                }),
            );
        } else {
            info!(
                "User `{}` did not request mojang info, but tried to log in.",
                user_id
            );
            send_message(
                &session.addr,
                ClientPacket::Error {
                    message: ClientError::MojangRequestMissing,
                },
                "mojang info missing"
            );
        }
    }
}
