use crate::chat::{ChatServer, ClientPacket, InternalId, SuccessReason, User};

use crate::error::*;
use log::*;

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

        if let Err(err) = session
            .addr
            .do_send(ClientPacket::MojangInfo { session_hash })
        {
            warn!("Could not send mojang info to user `{}`: {}", user_id, err);
        }
    }

    pub(super) fn login_mojang(
        &mut self,
        user_id: InternalId,
        info: User,
        ctx: &mut Context<Self>,
    ) {
        fn send_login_failed(
            user_id: InternalId,
            err: Error,
            session: &Recipient<ClientPacket>,
            _ctx: &mut Context<ChatServer>,
        ) {
            warn!("Could not authenticate user `{}`: {}", user_id, err);
            session
                .do_send(ClientPacket::Error {
                    message: ClientError::LoginFailed,
                })
                .ok();
        }

        let session = self
            .connections
            .get(&user_id)
            .expect("could not find connection");

        if session.is_logged_in() {
            info!("User `{}` tried to log in multiple times.", user_id);
            session
                .addr
                .do_send(ClientPacket::Error {
                    message: ClientError::AlreadyLoggedIn,
                })
                .ok();
            return;
        }

        if let Some(session_hash) = &session.session_hash {
            match authenticate(&info.name, session_hash) {
                Ok(fut) => {
                    fut.into_actor(self)
                        .then(move |res, actor, ctx| {
                            match res {
                                Ok(ref mojang_info)
                                    if Uuid::from_str(&mojang_info.id)
                                        .expect("got invalid uuid from mojang :()")
                                        == info.uuid =>
                                {
                                    info!(
                                        "User `{}` has uuid `{}` and username `{}`",
                                        user_id, mojang_info.id, mojang_info.name
                                    );

                                    if let Some(session) = actor.connections.get_mut(&user_id) {
                                        actor.ids.entry(info.name.as_str().into()).or_default().insert(user_id);
                                        session.user = Some(info);

                                        if let Err(err) =
                                            session.addr.do_send(ClientPacket::Success {
                                                reason: SuccessReason::Login,
                                            })
                                        {
                                            info!(
                                                "Could not send login success to `{}`: {}",
                                                user_id, err
                                            );
                                        }
                                    }
                                }
                                Ok(_) => {
                                    let session = actor.connections.get(&user_id).unwrap();
                                    send_login_failed(
                                        user_id,
                                        ClientError::InvalidId.into(),
                                        &session.addr,
                                        ctx,
                                    )
                                }
                                Err(err) => {
                                    let session = actor.connections.get(&user_id).unwrap();
                                    send_login_failed(user_id, err, &session.addr, ctx)
                                }
                            }
                            fut::ok(())
                        })
                        .spawn(ctx);
                }
                Err(err) => send_login_failed(user_id, err, &session.addr, ctx),
            }
        } else {
            info!(
                "User `{}` did not request mojang info, but tried to log in.",
                user_id
            );
            session
                .addr
                .do_send(ClientPacket::Error {
                    message: ClientError::MojangRequestMissing,
                })
                .ok();
        }
    }
}
