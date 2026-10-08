mod connect;
mod handler;
mod id;
mod packet;
mod session;

pub use id::*;
pub use session::Frame;

use packet::*;

use crate::api::{Api, RoleDefinition};
use crate::auth::{Authenticator, UserInfo};
use crate::config::Config;
use crate::entity::user;
use crate::error::ClientError;
use crate::ip::RealIp;
use crate::message::{MessageValidator, RateLimiter};
use crate::moderation::Moderation;
use crate::store::Store;
use log::*;

use actix::*;
use actix_web::{web, HttpRequest, HttpResponse};
use rand::{rngs::SysRng, SeedableRng};
use rand_hc::Hc128Rng;
use serde::Serialize;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use uuid::Uuid;

const MAX_FRAME_SIZE: usize = 64 * 1024;
/// Outgoing login requests at once; a restart reconnects everyone at the same time.
const CONCURRENT_LOGINS: usize = 64;
const ROLE_REFRESH: Duration = Duration::from_secs(600);

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

pub type UserId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Account,
    Mojang,
}

#[derive(Debug, Clone)]
pub struct Identity {
    pub id: UserId,
    pub kind: Kind,
    pub name: String,
    /// The Minecraft account shown for this user, nil if unknown.
    pub uuid: Uuid,
}

impl Identity {
    fn of(model: &user::Model) -> Identity {
        Identity {
            id: model.id,
            kind: if model.account.is_some() { Kind::Account } else { Kind::Mojang },
            name: model.name.clone(),
            uuid: model
                .minecraft_uuid
                .or(model.linked_minecraft_uuid)
                .unwrap_or_else(Uuid::nil),
        }
    }

    fn info(&self) -> UserInfo {
        UserInfo {
            name: self.name.clone(),
            uuid: self.uuid,
        }
    }
}

pub struct ChatServer {
    connections: HashMap<InternalId, Connection>,
    users: HashMap<UserId, OnlineUser>,
    /// Everyone seen since startup.
    directory: HashMap<UserId, Identity>,

    store: Addr<Store>,
    api: Arc<Api>,
    logins: Arc<Semaphore>,
    roles: HashMap<String, RoleDefinition>,
    rng: Hc128Rng,
    authenticator: Option<Authenticator>,
    validator: MessageValidator,
    moderation: Moderation,
    config: Config,

    current_internal_user_id: u64,
}

impl ChatServer {
    pub fn new(config: Config, store: Addr<Store>) -> ChatServer {
        ChatServer {
            connections: HashMap::new(),
            users: HashMap::new(),
            directory: HashMap::new(),

            store,
            api: Arc::new(Api::new(&config.api)),
            logins: Arc::new(Semaphore::new(CONCURRENT_LOGINS)),
            roles: HashMap::new(),
            rng: Hc128Rng::try_from_rng(&mut SysRng).expect("could not initialize hc128 rng"),
            authenticator: Authenticator::new(&config.auth),
            validator: MessageValidator::new(config.message.clone()),
            moderation: Moderation::new(config.moderation.clone())
                .expect("could not start moderation"),
            config,

            current_internal_user_id: 0,
        }
    }

    fn send(&self, id: InternalId, packet: ClientPacket) -> bool {
        self.connections
            .get(&id)
            .is_some_and(|connection| send_message(connection, packet))
    }

    fn send_error(&self, id: InternalId, error: ClientError) {
        self.send(id, ClientPacket::error(error));
    }

    /// The logged in user of a connection, or `NotLoggedIn` sent back.
    fn logged_in(&self, id: InternalId) -> Option<UserId> {
        match self.connections.get(&id)?.login {
            Login::User(user) => Some(user),
            _ => {
                self.send_error(id, ClientError::NotLoggedIn);
                None
            }
        }
    }

    fn identity(&self, user: UserId) -> &Identity {
        self.directory.get(&user).expect("online users are in the directory")
    }

    /// Resolves the name of an online user the way v1 private messages address them:
    /// exact spelling before case-insensitive, Minecraft accounts before LiquidBounce Accounts.
    fn find_online(&self, name: &str) -> Option<UserId> {
        self.users
            .keys()
            .filter_map(|id| self.directory.get(id))
            .filter(|identity| identity.name.eq_ignore_ascii_case(name))
            .min_by_key(|identity| (identity.name != name, identity.kind != Kind::Mojang))
            .map(|identity| identity.id)
    }

    fn online_connections(&self, user: UserId) -> impl Iterator<Item = (InternalId, &Connection)> {
        self.users
            .get(&user)
            .into_iter()
            .flat_map(|online| online.connections.iter())
            .filter_map(|id| self.connections.get(id).map(|connection| (*id, connection)))
    }
}

// try_send would also fail on a full mailbox; only a closed one is a delivery failure.
fn send_message(connection: &Connection, message: ClientPacket) -> bool {
    if connection.addr.connected() {
        connection.addr.do_send(message.encode(connection.protocol));
        true
    } else {
        warn!("Could not send packet: mailbox closed");
        false
    }
}

impl Actor for ChatServer {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        self.refresh_roles(ctx);
        ctx.run_interval(ROLE_REFRESH, |actor, ctx| actor.refresh_roles(ctx));
    }
}

impl ChatServer {
    fn refresh_roles(&mut self, ctx: &mut Context<Self>) {
        let api = self.api.clone();
        ctx.spawn(async move { api.roles().await }.into_actor(self).map(|result, actor, _ctx| {
            match result {
                Ok(roles) => {
                    actor.roles = roles.into_iter().map(|role| (role.id.clone(), role)).collect();
                }
                Err(err) => warn!("Could not refresh roles: {}", err),
            }
        }));
    }

    fn is_staff(&self, user: UserId) -> bool {
        self.users.get(&user).is_some_and(|online| {
            online
                .roles
                .iter()
                .any(|role| self.roles.get(role).is_some_and(|role| role.is_staff))
        })
    }
}

impl Handler<Disconnect> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Disconnect, _ctx: &mut Context<Self>) {
        info!("User `{}` disconnected.", msg.id);
        let Some(connection) = self.connections.remove(&msg.id) else { return };
        if let Login::User(user) = connection.login {
            if let Some(online) = self.users.get_mut(&user) {
                online.connections.retain(|id| *id != msg.id);
                if online.connections.is_empty() {
                    self.users.remove(&user);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Login {
    Anonymous,
    Pending,
    User(UserId),
}

struct Connection {
    addr: Recipient<Frame>,
    ip: IpAddr,
    protocol: Protocol,
    session_hash: Option<String>,
    login: Login,
    allow_messages: bool,
}

struct OnlineUser {
    connections: Vec<InternalId>,
    rate_limiter: RateLimiter,
    roles: Vec<String>,
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
