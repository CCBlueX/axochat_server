use crate::config::Config;
use crate::error::*;
use log::*;

use actix::{Actor, Addr, Context, Handler, Message, Recipient, StreamHandler};
use actix_web::{ws, HttpRequest, HttpResponse};
use serde::Serialize;

use hashbrown::HashMap;

pub fn chat_route(req: &HttpRequest<ServerState>) -> actix_web::Result<HttpResponse> {
    ws::start(req, Session)
}

#[derive(Clone)]
pub struct ServerState {
    pub addr: Addr<ChatServer>,
}

struct Session;

impl Actor for Session {
    type Context = ws::WebsocketContext<Self, ServerState>;
}

impl StreamHandler<ws::Message, ws::ProtocolError> for Session {
    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
        debug!("Received message {:?}", msg);
    }
}

impl Handler<Packet> for Session {
    type Result = ();

    fn handle(&mut self, msg: Packet, ctx: &mut Self::Context) {
        ctx.binary(msg.0);
    }
}

#[derive(Message)]
struct Packet(Vec<u8>);

impl<T: Serialize> From<&T> for Packet {
    fn from(value: &T) -> Packet {
        Packet(serde_cbor::to_vec(value).expect("could not serialize packet"))
    }
}

pub struct ChatServer {
    connections: HashMap<usize, Recipient<Packet>>,
}

impl Default for ChatServer {
    fn default() -> ChatServer {
        ChatServer {
            connections: HashMap::new(),
        }
    }
}

impl Actor for ChatServer {
    type Context = Context<Self>;
}
