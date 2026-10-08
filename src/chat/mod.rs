mod connect;
mod handler;
mod id;
mod packet;
mod session;

pub use id::*;
pub use session::Frame;

use packet::*;

use crate::config::Config;
use log::*;

use actix::*;
use actix_web::{web, HttpRequest, HttpResponse};

use crate::auth::Authenticator;
use crate::ip::RealIp;
use crate::message::{MessageValidator, RateLimiter};
use crate::moderation::Moderation;
use rand::{rngs::SysRng, SeedableRng};
use rand_hc::Hc128Rng;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

const MAX_FRAME_SIZE: usize = 64 * 1024;

pub async fn chat_route(
    req: HttpRequest,
    stream: web::Payload,
    srv: web::Data<Addr<ChatServer>>,
    real_ip: web::Data<RealIp>,
) -> actix_web::Result<HttpResponse> {
    let ip = real_ip.of(&req);
    let (response, ws, messages) = actix_ws::handle(&req, stream)?;
    session::Session::create(|ctx| {
        ctx.add_stream(messages.max_frame_size(MAX_FRAME_SIZE));
        session::Session::new(InternalId::new(0), srv.get_ref().clone(), ws, ip)
    });
    Ok(response)
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

            rng: Hc128Rng::try_from_rng(&mut SysRng).expect("could not initialize hc128 rng"),
            authenticator: Authenticator::new(&config.auth),
            validator: MessageValidator::new(config.message.clone()),
            moderation: Moderation::new(config.moderation.clone())
                .expect("could not start moderation"),
            config,

            current_internal_user_id: 0,
        }
    }
}

// try_send would also fail on a full mailbox; only a closed one is a delivery failure.
fn send_message(session: &SessionState, message: ClientPacket, context: &str) -> bool {
    if session.addr.connected() {
        session.addr.do_send(message.encode(session.protocol));
        true
    } else {
        warn!("Could not send {} to user: mailbox closed", context);
        false
    }
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
                if let Some(user_session) = self.users.get_mut(&info.name) {
                    user_session.connections.remove(&msg.id);
                    if user_session.connections.is_empty() {
                        self.users.remove(&info.name);
                    }
                }
            }
        }
    }
}

struct SessionState {
    addr: Recipient<Frame>,
    ip: IpAddr,
    protocol: Protocol,
    session_hash: Option<String>,
    login_pending: bool,
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
#[rtype(result = "()")]
struct Disconnect {
    id: InternalId,
}

#[derive(Message)]
#[rtype(result = "()")]
struct ServerPacketId {
    user_id: InternalId,
    packet: ServerPacket,
}

#[derive(Message)]
#[rtype(result = "()")]
struct Malformed {
    user_id: InternalId,
    error: String,
}
