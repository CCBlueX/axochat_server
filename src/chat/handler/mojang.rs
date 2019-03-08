use crate::chat::{ChatServer, ClientPacket, Id};

use crate::error::*;
use log::*;

use crate::auth::{authenticate, UserInfo};
use actix::*;
use rand::RngCore;

impl ChatServer {
    pub(super) fn handle_request_mojang_info(&mut self, user_id: Id) {
        let session = self
            .connections
            .get(&user_id)
            .expect("could not find connection");

        let mut bytes = [0; 20];
        self.rng.fill_bytes(&mut bytes);
        // we'll just ignore one bit so we that don't have to deal with a '-' sign
        bytes[0] &= 0b0111_1111;

        let session_hash = crate::auth::encode_sha1_bytes(&bytes);

        if let Err(err) = session
            .addr
            .do_send(ClientPacket::MojangInfo { session_hash })
        {
            warn!(
                "Could not send mojang info to user `#{:x}`: {}",
                user_id, err
            );
        }
    }

    pub(super) fn login_mojang(&mut self, user_id: Id, info: UserInfo, ctx: &mut Context<Self>) {
        fn send_login_failed(
            user_id: Id,
            err: Error,
            session: &Recipient<ClientPacket>,
            ctx: &mut Context<ChatServer>,
        ) {
            warn!("Could not authenticate user `{:x}`: {}", user_id, err);
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
            info!("{:x} tried to log in multiple times.", user_id);
            session
                .addr
                .do_send(ClientPacket::Error(ClientError::AlreadyLoggedIn))
                .ok();
            return;
        } else if self.users.contains_key(&info.username) {
            info!("{:x} is already logged in as `{}`.", user_id, info.username);
            session
                .addr
                .do_send(ClientPacket::Error(ClientError::AlreadyLoggedIn))
                .ok();
            return;
        }

        if let Some(session_hash) = &session.session_hash {
            match authenticate(&info.username, session_hash) {
                Ok(fut) => {
                    fut.into_actor(self)
                        .then(move |res, actor, ctx| {
                            match res {
                                Ok(info) => {
                                    info!(
                                        "User with id `{:x}` has uuid `{}` and username `{}`",
                                        user_id, info.id, info.name
                                    );
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
        } else {
            info!(
                "{:x} did not request mojang info, but tried to log in.",
                user_id
            );
            session
                .addr
                .do_send(ClientPacket::Error(ClientError::MojangRequestMissing))
                .ok();
            return;
        }

        if let Some(session) = self.connections.get_mut(&user_id) {
            self.users.insert(info.username.clone(), user_id);
            session.info = Some(info);

            if let Err(err) = session.addr.do_send(ClientPacket::LoginSuccess) {
                info!("Could not send login success to `#{:x}`: {}", user_id, err);
            }
        }
    }
}
