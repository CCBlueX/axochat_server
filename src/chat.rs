use crate::config::Config;
use crate::error::*;
use log::*;

use crate::auth::authenticate;
use actix::{
    dev::{MessageResponse, ResponseChannel},
    *,
};
use actix_web::{ws, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};

use hashbrown::HashMap;
use rand::{rngs::OsRng, Rng, RngCore, SeedableRng};
use rand_hc::Hc128Rng;

pub fn chat_route(req: &HttpRequest<ServerState>) -> actix_web::Result<HttpResponse> {
    ws::start(req, Session { id: 0 })
}

#[derive(Clone)]
pub struct ServerState {
    pub addr: Addr<ChatServer>,
}

struct Session {
    id: usize,
}

impl Actor for Session {
    type Context = ws::WebsocketContext<Self, ServerState>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.state()
            .addr
            .send(Connect {
                addr: ctx.address().recipient(),
            })
            .into_actor(self)
            .then(|res, actor, ctx| {
                match res {
                    Ok(id) => {
                        actor.id = id;
                    }
                    Err(err) => {
                        warn!("Could not accept connection: {}", err);
                        ctx.stop();
                    }
                }
                fut::ok(())
            })
            .wait(ctx)
    }

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        ctx.state().addr.do_send(Disconnect { id: self.id });
        Running::Stop
    }
}

impl StreamHandler<ws::Message, ws::ProtocolError> for Session {
    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
        debug!("Received message {:?}", msg);
        match msg {
            ws::Message::Ping(msg) => ctx.pong(&msg),
            ws::Message::Pong(_msg) => {}
            ws::Message::Text(_msg) => {
                warn!("Can't decode text message sent by client.");
            }
            ws::Message::Binary(msg) => {
                match serde_cbor::from_slice::<ServerPacket>(msg.as_ref()) {
                    Ok(packet) => ctx
                        .state()
                        .addr
                        .send(ServerPacketId {
                            user_id: self.id,
                            packet,
                        })
                        .into_actor(self)
                        .map_err(|err, _actor, _ctx| {
                            warn!("Could not decode packet: {}", err);
                        })
                        .wait(ctx),
                    Err(err) => {
                        warn!("Could not decode packet: {}", err);
                    }
                };
            }
            ws::Message::Close(Some(reason)) => {
                info!(
                    "Connection with id {:x} closed; code: {:?}, reason: {:?}",
                    self.id, reason.code, reason.description
                );
                ctx.stop();
            }
            ws::Message::Close(None) => {
                info!("Connection with id {:x} closed.", self.id);
                ctx.stop();
            }
        }
    }
}

impl Handler<ClientPacket> for Session {
    type Result = ();

    fn handle(&mut self, msg: ClientPacket, ctx: &mut Self::Context) {
        let bytes = serde_cbor::to_vec(&msg).expect("could not encode message");
        ctx.binary(bytes);
    }
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

#[derive(Message)]
#[rtype(usize)]
struct Connect {
    addr: Recipient<ClientPacket>,
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

impl Handler<Connect> for ChatServer {
    type Result = usize;

    fn handle(&mut self, msg: Connect, ctx: &mut Context<Self>) -> usize {
        use hashbrown::hash_map::Entry;

        let session_hash = {
            let mut bytes = [0; 20];
            self.rng.fill_bytes(&mut bytes);
            // we'll just ignore one bit so we that don't have to deal with a '-' sign
            bytes[0] &= 0b0111_1111;

            crate::auth::encode_sha1_bytes(&bytes)
        };

        msg.addr
            .send(ClientPacket::ServerInfo {
                session_hash: session_hash.clone(),
            })
            .into_actor(self)
            .then(|res, _actor, ctx| {
                match res {
                    Ok(()) => {}
                    Err(err) => {
                        warn!("Could not send session hash: {}", err);
                        ctx.stop();
                    }
                }
                fut::ok(())
            })
            .wait(ctx);

        loop {
            let id = self.rng.gen();
            match self.connections.entry(id) {
                Entry::Occupied(_) => {}
                Entry::Vacant(v) => {
                    v.insert(SessionState {
                        addr: msg.addr.clone(),
                        session_hash,
                        username: String::new(),
                        anonymous: true,
                    });
                    debug!("User with id {:x} joined the chat.", id);
                    return id;
                }
            }
        }
    }
}

impl Handler<Disconnect> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Disconnect, _ctx: &mut Context<Self>) -> () {
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
                let session = self
                    .connections
                    .get(&user_id)
                    .expect("could not find connection");
                if !session.username.is_empty() {
                    debug!("{:x} tried to log in multiple times.", user_id);
                    return;
                }

                match authenticate(&username, &session.session_hash) {
                    Ok(fut) => {
                        fut.into_actor(self)
                            .then(move |res, _actor, ctx| {
                                match res {
                                    Ok(info) => {
                                        info!(
                                            "User with id `{:x}` has uuid `{}` and username `{}`",
                                            user_id, info.id, info.name
                                        );
                                    }
                                    Err(err) => {
                                        warn!(
                                            "Could not authenticate user `{:x}`: {}",
                                            user_id, err
                                        );
                                        ctx.stop();
                                    }
                                }
                                fut::ok(())
                            })
                            .wait(ctx);
                    }
                    Err(err) => {
                        warn!(
                            "Could not authenticate user `{}` with id `{:x}`: {}",
                            username, user_id, err
                        );
                        ctx.stop();
                    }
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
                    debug!("{:x} tried to send message, but is not logged in.", user_id);
                    return;
                }

                let author_name = if session.anonymous {
                    None
                } else {
                    Some(session.username.clone())
                };

                let client_packet = ClientPacket::Message {
                    author_id: user_id,
                    author_name,
                    content: content,
                };
                for session in self.connections.values() {
                    // check if user is actually logged in
                    if !session.username.is_empty() {
                        if let Err(err) = session.addr.do_send(client_packet.clone()) {
                            warn!("Could not send message to client: {}", err);
                        }
                    }
                }
            }
            ServerPacket::PrivateMessage { receiver_id, content } => {
                info!("{:x} has written to `{:x}`.", user_id, receiver_id);
                debug!("{:x} has written `{}` to `{:x}`.", user_id, content, receiver_id);
                let sender_session = self
                    .connections
                    .get(&user_id)
                    .expect("could not find connection");
                if sender_session.username.is_empty() {
                    debug!("{:x} tried to send private message, but is not logged in.", user_id);
                    return;
                }
                let receiver_session = self
                    .connections
                    .get(&receiver_id)
                    .expect("could not find connection");

                let author_name = if sender_session.anonymous {
                    None
                } else {
                    Some(sender_session.username.clone())
                };

                // check if user is actually logged in
                if !receiver_session.username.is_empty() {
                    let client_packet = ClientPacket::PrivateMessage {
                        author_id: user_id,
                        author_name,
                        content: content,
                    };
                    if let Err(err) = receiver_session.addr.do_send(client_packet) {
                        warn!("Could not send private message to client: {}", err);
                    }
                }
            }
        }
    }
}
