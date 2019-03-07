use super::{ChatServer, ClientPacket, ServerPacket, ServerPacketId};

use crate::error::*;
use log::*;

use crate::auth::authenticate;
use actix::*;

impl Handler<ServerPacketId> for ChatServer {
    type Result = ();

    fn handle(
        &mut self,
        ServerPacketId { user_id, packet }: ServerPacketId,
        ctx: &mut Context<Self>,
    ) {
        match packet {
            ServerPacket::Login {
                anonymous,
                username,
            } => {
                fn send_login_failed(
                    user_id: usize,
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
                } else if self.users.contains_key(&username) {
                    info!("{:x} is already logged in as `{}`.", user_id, username);
                    session
                        .addr
                        .do_send(ClientPacket::Error(ClientError::AlreadyLoggedIn))
                        .ok();
                    return;
                }

                match authenticate(&username, &session.session_hash) {
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

                if let Some(session) = self.connections.get_mut(&user_id) {
                    self.users.insert(username.clone(), user_id);
                    session.username = username;
                    session.anonymous = anonymous;
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
            ServerPacket::PrivateMessage {
                receiver_id,
                content,
            } => {
                info!("{:x} has written to `{:x}`.", user_id, receiver_id);
                debug!(
                    "{:x} has written `{}` to `{:x}`.",
                    user_id, content, receiver_id
                );
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
                let receiver_session = match self.connections.get(&receiver_id) {
                    Some(ses) => ses,
                    None => {
                        debug!(
                            "{:x} tried to write to non-existing user `{:x}`.",
                            user_id, receiver_id
                        );
                        return;
                    }
                };

                if !receiver_session.is_logged_in() {
                    let client_packet = ClientPacket::PrivateMessage {
                        author_id: user_id,
                        author_name: sender_session.username_opt(),
                        content,
                    };
                    if let Err(err) = receiver_session.addr.do_send(client_packet) {
                        warn!("Could not send private message to client: {}", err);
                    }
                }
            }
        }
    }
}
