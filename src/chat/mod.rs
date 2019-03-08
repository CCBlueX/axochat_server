mod connect;
mod handler;
mod session;

use crate::config::Config;
use crate::error::*;

use actix::{
    dev::{MessageResponse, ResponseChannel},
    *,
};
use actix_web::{ws, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::auth::{Authenticator, UserInfo};
use crate::message::{MessageValidator, RateLimiter};
use hashbrown::HashMap;
use rand::{rngs::OsRng, SeedableRng};
use rand_hc::Hc128Rng;

pub fn chat_route(req: &HttpRequest<ServerState>) -> actix_web::Result<HttpResponse> {
    ws::start(req, session::Session::new(Id(0)))
}

#[derive(Clone)]
pub struct ServerState {
    pub addr: Addr<ChatServer>,
}

pub struct ChatServer {
    connections: HashMap<Id, SessionState>,
    users: HashMap<String, Id>,
    rng: rand_hc::Hc128Rng,
    authenticator: Option<Authenticator>,
    validator: MessageValidator,
    config: Config,
}

impl ChatServer {
    pub fn new(config: Config) -> ChatServer {
        ChatServer {
            connections: HashMap::new(),
            users: HashMap::new(),
            rng: {
                let os_rng = OsRng::new().expect("could not initialize os rng");
                Hc128Rng::from_rng(os_rng).expect("could not initialize hc128 rng")
            },
            authenticator: config
                .auth
                .as_ref()
                .map(|auth| Authenticator::new(&auth).expect("could not initialize authenticator")),
            validator: MessageValidator::new(config.message.clone()),
            config,
        }
    }

    fn get_connection(&self, user: &AtUser) -> Option<&SessionState> {
        match user {
            AtUser::Id(id) => self.connections.get(&id),
            AtUser::Name(name) => {
                let id = self.users.get(name)?;
                self.connections.get(&id)
            }
        }
    }
}

impl Actor for ChatServer {
    type Context = Context<Self>;
}

impl Handler<Disconnect> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Disconnect, _ctx: &mut Context<Self>) {
        if let Some(session) = self.connections.remove(&msg.id) {
            if let Some(info) = session.info {
                self.users.remove(&info.username);
            }
        }
    }
}

struct SessionState {
    addr: Recipient<ClientPacket>,
    session_hash: Option<String>,
    rate_limiter: RateLimiter,
    info: Option<UserInfo>,
}

impl SessionState {
    pub fn is_logged_in(&self) -> bool {
        self.info.is_some()
    }

    pub fn username_opt(&self) -> Option<String> {
        match self.info {
            Some(ref info) if !info.anonymous => Some(info.username.clone()),
            _ => None,
        }
    }
}

#[derive(Message)]
struct Disconnect {
    id: Id,
}

/// A clientbound packet
#[derive(Message, Serialize, Clone)]
enum ClientPacket {
    MojangInfo {
        session_hash: String,
    },
    NewJWT(String),
    LoginSuccess,
    Message {
        author_id: Id,
        author_name: Option<String>,
        content: String,
    },
    PrivateMessage {
        author_id: Id,
        author_name: Option<String>,
        content: String,
    },
    Error(ClientError),
}

/// A serverbound packet
#[derive(Message, Deserialize)]
enum ServerPacket {
    RequestMojangInfo,
    LoginMojang(UserInfo),
    LoginJWT(String),
    RequestJWT,
    Message { content: String },
    PrivateMessage { receiver: AtUser, content: String },
}

#[derive(Message)]
struct ServerPacketId {
    user_id: Id,
    packet: ServerPacket,
}

#[derive(Deserialize)]
enum AtUser {
    Id(Id),
    Name(String),
}

impl fmt::Display for AtUser {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AtUser::Id(id) => id.fmt(f),
            AtUser::Name(name) => write!(f, "{}", name),
        }
    }
}

#[derive(Serialize, Deserialize, Copy, Clone, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct Id(u32);

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "#{:x}", self.0)
    }
}

impl<A, M> MessageResponse<A, M> for Id
where
    A: Actor,
    M: Message<Result = Id>,
{
    fn handle<R: ResponseChannel<M>>(self, _: &mut A::Context, tx: Option<R>) {
        if let Some(tx) = tx {
            tx.send(self);
        }
    }
}
