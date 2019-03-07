mod connect;
mod handler;
mod session;

use crate::error::*;

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
