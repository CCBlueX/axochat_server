use super::{ChatServer, ClientPacket, Id, ServerPacket, ServerPacketId};

use crate::error::*;
use log::*;

use crate::auth::authenticate;
use actix::*;
use rand::RngCore;

impl Handler<ServerPacketId> for ChatServer {
    type Result = ();

    fn handle(
        &mut self,
        ServerPacketId { user_id, packet }: ServerPacketId,
        ctx: &mut Context<Self>,
    ) {
        match packet {
            ServerPacket::RequestMojangInfo => {
                let mut bytes = [0; 20];
                self.rng.fill_bytes(&mut bytes);
                // we'll just ignore one bit so we that don't have to deal with a '-' sign
                bytes[0] &= 0b0111_1111;

                let session_hash = crate::auth::encode_sha1_bytes(&bytes);

                let session = self
                    .connections
                    .get(&user_id)
                    .expect("could not find connection");
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
            ServerPacket::LoginMojang(info) => {
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
            ServerPacket::RequestJWT => {
                let session = self
                    .connections
                    .get(&user_id)
                    .expect("could not find connection");
                if let Some(auth) = &self.authenticator {
                    if let Some(info) = &session.info {
                        let token = match auth.new_token(info) {
                            Ok(token) => token,
                            Err(err) => {
                                warn!(
                                    "Could not create new token for user `#{:x}`: {}",
                                    user_id, err
                                );
                                session
                                    .addr
                                    .do_send(ClientPacket::Error(ClientError::Internal))
                                    .ok();
                                return;
                            }
                        };

                        if let Err(err) = session.addr.do_send(ClientPacket::NewJWT(token)) {
                            warn!(
                                "Could not send mojang info to user `#{:x}`: {}",
                                user_id, err
                            );
                        }
                    } else {
                        info!(
                            "User `#{:x}` tried to get JWT but is not logged in.",
                            user_id
                        );
                        session
                            .addr
                            .do_send(ClientPacket::Error(ClientError::NotLoggedIn))
                            .ok();
                    }
                } else {
                    info!("User `#{:x}` tried to request not supported JWT", user_id);
                    session
                        .addr
                        .do_send(ClientPacket::Error(ClientError::NotSupported))
                        .ok();
                }
            }
            ServerPacket::LoginJWT(jwt) => {
                let session = self
                    .connections
                    .get_mut(&user_id)
                    .expect("could not find connection");
                if let Some(auth) = &self.authenticator {
                    match auth.auth(&jwt) {
                        Ok(info) => {
                            self.users.insert(info.username.clone(), user_id);
                            session.info = Some(info);
                            if let Err(err) = session.addr.do_send(ClientPacket::LoginSuccess) {
                                info!("Could not send login success to `#{:x}`: {}", user_id, err);
                            }
                        }
                        Err(err) => {
                            info!("Login of user `#{:x}` using JWT failed: {}", user_id, err);
                            session
                                .addr
                                .do_send(ClientPacket::Error(ClientError::LoginFailed))
                                .ok();
                        }
                    };
                } else {
                    info!("User `#{:x}` tried to request not supported JWT", user_id);
                    session
                        .addr
                        .do_send(ClientPacket::Error(ClientError::NotSupported))
                        .ok();
                }
            }
            ServerPacket::Message { content } => {
                info!("{:x} has written `{}`.", user_id, content);
                let session = self
                    .connections
                    .get_mut(&user_id)
                    .expect("could not find connection");
                if !session.is_logged_in() {
                    info!("{:x} is not logged in.", user_id);
                    session
                        .addr
                        .do_send(ClientPacket::Error(ClientError::NotLoggedIn))
                        .ok();
                    return;
                }
                if session.rate_limiter.check_new_message() {
                    info!("{:x} tried to send message, but was rate limited.", user_id);
                    session
                        .addr
                        .do_send(ClientPacket::Error(ClientError::RateLimited))
                        .ok();
                    return;
                }

                let client_packet = ClientPacket::Message {
                    author_id: user_id,
                    author_name: session.username_opt(),
                    content,
                };
                for session in self.connections.values() {
                    if !session.is_logged_in() {
                        if let Err(err) = session.addr.do_send(client_packet.clone()) {
                            warn!("Could not send message to client: {}", err);
                        }
                    }
                }
            }
            ServerPacket::PrivateMessage { receiver, content } => {
                info!("{:x} has written to `{}`.", user_id, receiver);
                debug!("{:x} has written `{}` to `{}`.", user_id, content, receiver);
                let sender_session = self
                    .connections
                    .get(&user_id)
                    .expect("could not find connection");
                if !sender_session.is_logged_in() {
                    info!("{:x} is not logged in.", user_id);
                    sender_session
                        .addr
                        .do_send(ClientPacket::Error(ClientError::NotLoggedIn))
                        .ok();
                    return;
                }
                let receiver_session = match self.get_connection(&receiver) {
                    Some(ses) => ses,
                    None => {
                        debug!(
                            "{:x} tried to write to non-existing user `{}`.",
                            user_id, receiver
                        );
                        return;
                    }
                };

                match &receiver_session.info {
                    Some(info) if info.allow_messages => {
                        let client_packet = ClientPacket::PrivateMessage {
                            author_id: user_id,
                            author_name: sender_session.username_opt(),
                            content,
                        };
                        if let Err(err) = receiver_session.addr.do_send(client_packet) {
                            warn!("Could not send private message to client: {}", err);
                        } else {
                            return;
                        }
                    }
                    _ => {}
                }

                sender_session
                    .addr
                    .do_send(ClientPacket::Error(ClientError::PrivateMessageNotAccepted))
                    .ok();
            }
        }
    }
}
