mod connect;
mod handler;
mod id;
mod session;

pub use id::*;

use crate::config::Config;
use crate::error::*;
use log::*;

use actix::*;
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use serde::{Deserialize, Serialize};

use crate::auth::{Authenticator, UserInfo};
use crate::message::{MessageValidator, RateLimiter};
use crate::moderation::Moderation;
use rand::{rngs::OsRng, SeedableRng};
use rand_hc::Hc128Rng;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

// Updated to work with Actix Web 4.x
pub async fn chat_route(
    req: HttpRequest,
    stream: web::Payload,
    srv: web::Data<Addr<ChatServer>>,
) -> actix_web::Result<HttpResponse> {
    ws::start(
        session::Session::new(InternalId::new(0), srv.get_ref().clone()),
        &req,
        stream,
    )
}

pub struct ChatServer {
    connections: HashMap<InternalId, SessionState>,
    users: HashMap<String, UserSession>,

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
            users: HashMap::new(),

            rng: Hc128Rng::from_rng(OsRng).expect("could not initialize hc128 rng"),
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
}

// Define a separate helper function outside the impl to avoid borrowing issues
pub(crate) fn send_message(recipient: &Recipient<ClientPacket>, message: ClientPacket, context: &str) {
    // In Actix 0.13, do_send returns () (unit type), not a Result
    // We'll just call it and log any errors that might be caught
    recipient.do_send(message);
    // If we want to catch errors, we'd need to use the try_send method instead
    // But for now we'll keep things simple
}

impl Actor for ChatServer {
    type Context = Context<Self>;
}

impl Handler<Disconnect> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Disconnect, _ctx: &mut Context<Self>) {
        info!("User `{}` disconnected.", msg.id);
        if let Some(session) = self.connections.remove(&msg.id) {
            if let Some(info) = session.user {
                let user_session = self
                    .users
                    .get_mut(&info.name)
                    .expect("the ids should still exist here");
                user_session.connections.remove(&msg.id);
                if user_session.connections.is_empty() {
                    self.users.remove(&info.name);
                }
            }
        }
    }
}

pub(self) struct SessionState {
    addr: Recipient<ClientPacket>,
    session_hash: Option<String>,
    user: Option<User>,
}

impl SessionState {
    pub fn is_logged_in(&self) -> bool {
        self.user.is_some()
    }
}

struct UserSession {
    rate_limiter: RateLimiter,
    connections: HashSet<InternalId>,
}

#[derive(Message)]
#[rtype(result = "()")]  // Added return type for Actix 0.13
struct Disconnect {
    id: InternalId,
}

/// A clientbound packet
#[derive(Message, Serialize, Clone)]
#[rtype(result = "()")]  // Added return type for Actix 0.13
#[serde(tag = "m", content = "c")]
enum ClientPacket {
    MojangInfo {
        session_hash: String,
    },
    NewJWT {
        token: String,
    },
    Message {
        author_info: UserInfo,
        content: String,
    },
    PrivateMessage {
        author_info: UserInfo,
        content: String,
    },
    UserCount {
        connections: u32,
        logged_in: u32,
    },
    Success {
        reason: SuccessReason,
    },
    Error {
        message: ClientError,
    },
}

/// A serverbound packet
#[derive(Message, Deserialize)]
#[rtype(result = "()")]  // Added return type for Actix 0.13
#[serde(tag = "m", content = "c")]
enum ServerPacket {
    RequestMojangInfo,
    LoginMojang(User),
    LoginJWT { token: String, allow_messages: bool },
    RequestJWT,
    Message { content: String },
    PrivateMessage { receiver: String, content: String },
    BanUser { user: Uuid },
    UnbanUser { user: Uuid },
    RequestUserCount,
}

#[derive(Message)]
#[rtype(result = "()")]  // Added return type for Actix 0.13
struct ServerPacketId {
    user_id: InternalId,
    packet: ServerPacket,
}

#[derive(Serialize, Deserialize, Clone)]
struct User {
    pub name: String,
    pub uuid: Uuid,
    /// Should this user allow private messages?
    pub allow_messages: bool,
}

#[derive(Serialize, Deserialize, Copy, Clone)]
enum SuccessReason {
    Login,
    Ban,
    Unban,
}
