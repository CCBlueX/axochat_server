use crate::config::Config;
use crate::error::*;
use log::*;

use actix::{
    dev::{MessageResponse, ResponseChannel},
    *,
};
use actix_web::{ws, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use crate::auth::authenticate;

use hashbrown::HashMap;
use rand::{rngs::OsRng, Rng, RngCore, SeedableRng};
use rand_hc::Hc128Rng;

pub fn chat_route(req: &HttpRequest<ServerState>) -> actix_web::Result<HttpResponse> {
    ws::start(
        req,
        Session {
            id: 0,
        },
    )
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
    ServerInfo { session_hash: String },
    Message { author_id: usize, content: String },
}

/// A serverbound packet
#[derive(Message, Deserialize)]
enum ServerPacket {
    Login { username: String, anonymous: bool },
    Message { content: String },
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
            const HEX_ALPHABET: [char; 16] = [
                '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
            ];

            let mut bytes = [0; 20];
            self.rng.fill_bytes(&mut bytes);
            // we'll just ignore one bit so we that don't have to deal with a '-' sign
            bytes[0] &= 0b0111_1111;

            let mut session_hash = String::with_capacity(20);
            for &byte in bytes.into_iter().skip_while(|&&b| b == 0) {
                session_hash.push(HEX_ALPHABET[(byte >> 4) as usize]);
                session_hash.push(HEX_ALPHABET[(byte & 0b1111) as usize]);
            }

            session_hash
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
                let session = self.connections.get(&user_id).expect("could not find connection");

                match authenticate(&username, &session.session_hash) {
                    Ok(fut) => {
                        fut.into_actor(self)
                            .then(move |res, _actor, ctx| {
                                match res {
                                    Ok(info) => {
                                        info!("User with id `{:x}` has uuid `{}` and username `{}`", user_id, info.id, info.name);
                                    }
                                    Err(err) => {
                                        warn!("Could not authenticate user `{:x}`: {}", user_id, err);
                                        ctx.stop();
                                    }
                                }
                                fut::ok(())
                            })
                            .wait(ctx);
                    },
                    Err(err) => {
                        warn!("Could not authenticate user `{}` with id `{:x}`: {}", username, user_id, err);
                        ctx.stop();
                    }
                }

                let session = self.connections.get_mut(&user_id).expect("could not find connection");
                session.username = username;
                session.anonymous = anonymous;
            }
            ServerPacket::Message { content } => {
                info!("{:x} has written `{}`.", user_id, content);
                let client_packet = ClientPacket::Message {
                    author_id: user_id,
                    content: content,
                };
                for session in self.connections.values() {
                    if let Err(err) = session.addr.do_send(client_packet.clone()) {
                        warn!("Could not send message to client: {}", err);
                    }
                }
            }
        }
    }
}
