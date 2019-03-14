mod connect;
mod handler;
mod id;
mod session;

pub use id::*;

use crate::config::Config;
use crate::error::*;

use actix::*;
use actix_web::{ws, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};

use crate::auth::{Authenticator, UserInfo};
use crate::message::{MessageValidator, RateLimiter};
use crate::moderation::Moderation;
use hashbrown::HashMap;
use rand::{rngs::OsRng, SeedableRng};
use rand_hc::Hc128Rng;

pub fn chat_route(req: &HttpRequest<ServerState>) -> actix_web::Result<HttpResponse> {
    ws::start(req, session::Session::new(InternalId::new(0)))
}

#[derive(Clone)]
pub struct ServerState {
    pub addr: Addr<ChatServer>,
}

pub struct ChatServer {
    connections: HashMap<InternalId, SessionState>,
    ids: HashMap<Id, InternalId>,

    rng: rand_hc::Hc128Rng,
    authenticator: Option<Authenticator>,
    validator: MessageValidator,
    moderation: Moderation,
    config: Config,

    current_internal_user_id: u64,
}

impl ChatServer {
    pub fn new(config: Config) -> ChatServer {
        ChatServer {
            connections: HashMap::new(),
            ids: HashMap::new(),

            rng: {
                let os_rng = OsRng::new().expect("could not initialize os rng");
                Hc128Rng::from_rng(os_rng).expect("could not initialize hc128 rng")
            },
            authenticator: config
                .auth
                .as_ref()
                .map(|auth| Authenticator::new(&auth).expect("could not initialize authenticator")),
            validator: MessageValidator::new(config.message.clone()),
            moderation: Moderation::new(config.moderation.clone())
                .expect("could not start moderation"),
            config,

            current_internal_user_id: 0,
        }
    }

    pub(self) fn connection_by_id(&self, id: &Id) -> Option<&SessionState> {
        let internal_id = self.ids.get(id)?;
        self.connections.get(&internal_id)
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
                self.ids.remove(&info.username.as_str().into());
            }
        }
    }
}

pub(self) struct SessionState {
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
    id: InternalId,
}

/// A clientbound packet
#[derive(Message, Serialize, Clone)]
#[serde(tag = "m", content = "c")]
enum ClientPacket {
    MojangInfo {
        session_hash: String,
    },
    NewJWT(String),
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
    Success,
    Error(ClientError),
}

/// A serverbound packet
#[derive(Message, Deserialize)]
#[serde(tag = "m", content = "c")]
enum ServerPacket {
    RequestMojangInfo,
    LoginMojang(UserInfo),
    LoginJWT(String),
    RequestJWT,
    Message { content: String },
    PrivateMessage { receiver: Id, content: String },
    BanUser(Id),
    UnbanUser(Id),
}

#[derive(Message)]
struct ServerPacketId {
    user_id: InternalId,
    packet: ServerPacket,
}
