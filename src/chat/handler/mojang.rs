use crate::chat::{ChatServer, ClientPacket, InternalId, User};

use crate::error::*;
use log::*;

use crate::auth::authenticate;
use actix::*;
use rand::RngCore;
use std::str::FromStr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
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
            ctx: &mut Context<ChatServer>,
        ) {
            warn!("Could not authenticate user `{}`: {}", user_id, err);
            session
                .do_send(ClientPacket::Error(ClientError::LoginFailed))
                .ok();
            ctx.stop();
        }

        let session = self
            .connections
            .get(&user_id)
            .expect("could not find connection");

        if session.is_logged_in() {
            info!("User `{}` tried to log in multiple times.", user_id);
            session
                .addr
                .do_send(ClientPacket::Error(ClientError::AlreadyLoggedIn))
                .ok();
            return;
        } else if self.ids.contains_key(&info.uuid.into()) {
            info!(
                "User `{}` is already logged in as `{}`.",
                user_id, info.name
            );
            session
                .addr
                .do_send(ClientPacket::Error(ClientError::AlreadyLoggedIn))
                .ok();
            return;
        }

        if let Some(session_hash) = &session.session_hash {
            let logged_in = Arc::new(AtomicBool::new(false));
            let uuid = info.uuid;
            match authenticate(&info.name, session_hash) {
                Ok(fut) => {
                    let logged_in = Arc::clone(&logged_in);
                    fut.into_actor(self)
                        .then(move |res, actor, ctx| {
                            match res {
                                Ok(ref info)
                                    if Uuid::from_str(&info.id)
                                        .expect("got invalid uuid from mojang :()")
                                        == uuid =>
                                {
                                    info!(
                                        "User `{}` has uuid `{}` and username `{}`",
                                        user_id, info.id, info.name
                                    );
                                    logged_in.store(true, Ordering::Relaxed);
                                }
                                Ok(_) => {
                                    let session = actor.connections.get(&user_id).unwrap();
                                    send_login_failed(
                                        user_id,
                                        Error::AxoChat(ClientError::InvalidId),
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
                        .wait(ctx);
                }
                Err(err) => send_login_failed(user_id, err, &session.addr, ctx),
            }

            if logged_in.load(Ordering::Relaxed) {
                if let Some(session) = self.connections.get_mut(&user_id) {
                    self.ids.insert(info.uuid.into(), user_id);
                    session.user = Some(info);

                    if let Err(err) = session.addr.do_send(ClientPacket::Success) {
                        info!("Could not send login success to `{}`: {}", user_id, err);
                    }
                }
            }
        } else {
            info!(
                "User `{}` did not request mojang info, but tried to log in.",
                user_id
            );
            session
                .addr
                .do_send(ClientPacket::Error(ClientError::MojangRequestMissing))
                .ok();
        }
    }
}
