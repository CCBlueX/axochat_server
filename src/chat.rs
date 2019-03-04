use crate::config::Config;
use crate::error::*;
use log::*;

use actix::*;
use actix_web::{ws, HttpRequest, HttpResponse};
use serde::Serialize;

use hashbrown::HashMap;
use rand::Rng;

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

    fn started(&mut self, ctx: &mut Self::Context) {
        let addr = ctx.address();
        ctx.state()
            .addr
            .send(Connect { addr: addr.recipient() })
            .into_actor(self)
            .then(|res, _actor, ctx| {
                match res {
                    Ok(_) => {}
                    Err(err) => {
                        warn!("Could not accept connection: {}", err);
                        ctx.stop();
                    }
                }
                fut::ok(())
            })
            .wait(ctx)
    }
}

impl StreamHandler<ws::Message, ws::ProtocolError> for Session {
    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
        debug!("Received message {:?}", msg);
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
enum ClientPacket {

}

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
