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
use serde::{Deserialize, Serialize};

use crate::auth::{Authenticator, UserInfo};
use crate::message::{MessageValidator, RateLimiter};
use crate::moderation::Moderation;
use rand::{rngs::SysRng, SeedableRng};
use rand_hc::Hc128Rng;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub async fn chat_route(
    req: HttpRequest,
    stream: web::Payload,
    srv: web::Data<Addr<ChatServer>>,
) -> actix_web::Result<HttpResponse> {
    let (response, ws, messages) = actix_ws::handle(&req, stream)?;
    session::Session::create(|ctx| {
        ctx.add_stream(messages);
        session::Session::new(InternalId::new(0), srv.get_ref().clone(), ws)
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
pub(crate) fn send_message(recipient: &Recipient<ClientPacket>, message: ClientPacket, context: &str) -> bool {
    if recipient.connected() {
        recipient.do_send(message);
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
#[rtype(result = "()")]
struct Disconnect {
    id: InternalId,
}

/// A clientbound packet
#[derive(Message, Serialize, Clone)]
#[rtype(result = "()")]
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
#[rtype(result = "()")]
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
#[rtype(result = "()")]
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

#[cfg(test)]
mod tests {
    use super::{ClientError, ClientPacket, ServerPacket, SuccessReason, User, UserInfo};
    use serde_json::json;

    fn notch() -> UserInfo {
        UserInfo {
            name: "Notch".into(),
            uuid: "069a79f4-44e9-4726-a5be-fca90e38aaf5".parse().unwrap(),
        }
    }

    fn encode(packet: ClientPacket) -> serde_json::Value {
        serde_json::from_str(&serde_json::to_string(&packet).unwrap()).unwrap()
    }

    #[test]
    fn client_packets() {
        let author = json!({ "name": "Notch", "uuid": "069a79f4-44e9-4726-a5be-fca90e38aaf5" });
        let cases = [
            (
                ClientPacket::MojangInfo { session_hash: "88e16a1019277b15d58faf0541e11910eb756f6".into() },
                json!({ "m": "MojangInfo", "c": { "session_hash": "88e16a1019277b15d58faf0541e11910eb756f6" } }),
            ),
            (
                ClientPacket::NewJWT { token: "token".into() },
                json!({ "m": "NewJWT", "c": { "token": "token" } }),
            ),
            (
                ClientPacket::Message { author_info: notch(), content: "Hello, World!".into() },
                json!({ "m": "Message", "c": { "author_info": author, "content": "Hello, World!" } }),
            ),
            (
                ClientPacket::PrivateMessage { author_info: notch(), content: "Hello, User!".into() },
                json!({ "m": "PrivateMessage", "c": { "author_info": author, "content": "Hello, User!" } }),
            ),
            (
                ClientPacket::UserCount { connections: 623, logged_in: 531 },
                json!({ "m": "UserCount", "c": { "connections": 623, "logged_in": 531 } }),
            ),
            (
                ClientPacket::Success { reason: SuccessReason::Login },
                json!({ "m": "Success", "c": { "reason": "Login" } }),
            ),
            (
                ClientPacket::Success { reason: SuccessReason::Ban },
                json!({ "m": "Success", "c": { "reason": "Ban" } }),
            ),
            (
                ClientPacket::Success { reason: SuccessReason::Unban },
                json!({ "m": "Success", "c": { "reason": "Unban" } }),
            ),
            (
                ClientPacket::Error { message: ClientError::LoginFailed },
                json!({ "m": "Error", "c": { "message": "LoginFailed" } }),
            ),
        ];
        for (packet, expected) in cases {
            assert_eq!(encode(packet), expected);
        }
    }

    #[test]
    fn server_packets() {
        let decode = |value: serde_json::Value| -> ServerPacket { serde_json::from_value(value).unwrap() };

        assert!(matches!(decode(json!({ "m": "RequestMojangInfo" })), ServerPacket::RequestMojangInfo));
        assert!(matches!(decode(json!({ "m": "RequestJWT" })), ServerPacket::RequestJWT));
        assert!(matches!(decode(json!({ "m": "RequestUserCount" })), ServerPacket::RequestUserCount));
        assert!(matches!(
            decode(json!({ "m": "LoginMojang", "c": { "name": "Notch", "uuid": "069a79f444e94726a5befca90e38aaf5", "allow_messages": true } })),
            ServerPacket::LoginMojang(User { ref name, allow_messages: true, .. }) if name == "Notch"
        ));
        assert!(matches!(
            decode(json!({ "m": "LoginJWT", "c": { "token": "token", "allow_messages": false } })),
            ServerPacket::LoginJWT { ref token, allow_messages: false } if token == "token"
        ));
        assert!(matches!(
            decode(json!({ "m": "Message", "c": { "content": "Hello, World!" } })),
            ServerPacket::Message { ref content } if content == "Hello, World!"
        ));
        assert!(matches!(
            decode(json!({ "m": "PrivateMessage", "c": { "content": "Hello, Notch!", "receiver": "Notch" } })),
            ServerPacket::PrivateMessage { ref receiver, .. } if receiver == "Notch"
        ));
        assert!(matches!(
            decode(json!({ "m": "BanUser", "c": { "user": "069a79f4-44e9-4726-a5be-fca90e38aaf5" } })),
            ServerPacket::BanUser { .. }
        ));
        assert!(matches!(
            decode(json!({ "m": "UnbanUser", "c": { "user": "069a79f4-44e9-4726-a5be-fca90e38aaf5" } })),
            ServerPacket::UnbanUser { .. }
        ));
    }
}
