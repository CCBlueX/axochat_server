mod connect;
mod session;

use crate::config::Config;
use crate::error::*;
use log::*;

use crate::auth::authenticate;
use actix::*;
use actix_web::{ws, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};

use hashbrown::HashMap;
use rand::{rngs::OsRng, SeedableRng};
use rand_hc::Hc128Rng;

pub fn chat_route(req: &HttpRequest<ServerState>) -> actix_web::Result<HttpResponse> {
    ws::start(req, session::Session::new(0))
}

#[derive(Clone)]
pub struct ServerState {
    pub addr: Addr<ChatServer>,
}

pub struct ChatServer {
    connections: HashMap<usize, SessionState>,
    rng: rand_hc::Hc128Rng,
}

impl Default for ChatServer {
    fn default() -> ChatServer {
        ChatServer {
            connections: HashMap::new(),
            rng: {
                let os_rng = OsRng::new().expect("could not initialize os rng");
                Hc128Rng::from_rng(os_rng).expect("could not initialize hc128 rng")
            },
        }
    }
}

impl Actor for ChatServer {
    type Context = Context<Self>;
}

struct SessionState {
    addr: Recipient<ClientPacket>,
    session_hash: String,
    username: String,
    anonymous: bool,
}

impl SessionState {
    pub fn is_logged_in(&self) -> bool {
        !self.username.is_empty()
    }

    pub fn username_opt(&self) -> Option<String> {
        if self.anonymous {
            None
        } else {
            Some(self.username.clone())
        }
    }
}

#[derive(Message)]
struct Disconnect {
    id: usize,
}

/// A clientbound packet
#[derive(Message, Serialize, Clone)]
enum ClientPacket {
    ServerInfo {
        session_hash: String,
    },
    Message {
        author_id: usize,
        author_name: Option<String>,
        content: String,
    },
    PrivateMessage {
        author_id: usize,
        author_name: Option<String>,
        content: String,
    },
    Error(ClientError),
}

/// A serverbound packet
#[derive(Message, Deserialize)]
enum ServerPacket {
    Login { username: String, anonymous: bool },
    Message { content: String },
    PrivateMessage { receiver_id: usize, content: String },
}

#[derive(Message)]
struct ServerPacketId {
    user_id: usize,
    packet: ServerPacket,
}

impl Handler<Disconnect> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Disconnect, _ctx: &mut Context<Self>) {
        self.connections.remove(&msg.id);
    }
}

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
                if !session.is_logged_in() {
                    info!("{:x} tried to log in multiple times.", user_id);
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
                    session.username = username;
                    session.anonymous = anonymous;
                }
            }
            ServerPacket::Message { content } => {
                info!("{:x} has written `{}`.", user_id, content);
                let session = self
                    .connections
                    .get(&user_id)
                    .expect("could not find connection");
                if session.username.is_empty() {
                    info!("{:x} tried to send message, but is not logged in.", user_id);
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
                    info!(
                        "{:x} tried to send private message, but is not logged in.",
                        user_id
                    );
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
