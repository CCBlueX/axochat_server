use crate::config::Config;
use crate::error::*;
use log::*;

use actix::*;
use actix_web::{ws, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};

use hashbrown::HashMap;
use rand::Rng;

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
                    Ok(id) => actor.id = id,
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
                        .send(packet)
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

/// A clientbound packet
#[derive(Message, Serialize)]
enum ClientPacket {}

/// A serverbound packet
#[derive(Message, Deserialize)]
enum ServerPacket {}

pub struct ChatServer {
    connections: HashMap<usize, Recipient<ClientPacket>>,
    rng: rand::rngs::ThreadRng,
}

impl Default for ChatServer {
    fn default() -> ChatServer {
        ChatServer {
            connections: HashMap::new(),
            rng: rand::thread_rng(),
        }
    }
}

impl Actor for ChatServer {
    type Context = Context<Self>;
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

impl Handler<Connect> for ChatServer {
    type Result = usize;

    fn handle(&mut self, msg: Connect, _ctx: &mut Context<Self>) -> usize {
        use hashbrown::hash_map::Entry;

        loop {
            let id = self.rng.gen();
            match self.connections.entry(id) {
                Entry::Occupied(_) => {}
                Entry::Vacant(v) => {
                    v.insert(msg.addr.clone());
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

impl Handler<ServerPacket> for ChatServer {
    type Result = ();

    fn handle(&mut self, packet: ServerPacket, _ctx: &mut Context<Self>) {
        match packet {}
    }
}
