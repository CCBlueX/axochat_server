mod channel;
mod connect;
mod group;
mod party;
mod handler;
mod id;
mod packet;
mod session;
mod social;
mod world;

pub use id::*;
pub use session::Frame;

use packet::*;
use channel::Channel;
use group::{GroupId, Groups};
use party::Parties;
use world::{Location, Player};
use social::Social;

use crate::api::{Api, RoleDefinition};
use crate::config::Config;
use crate::entity::user;
use crate::error::ClientError;
use crate::ip::RealIp;
use crate::message::{ActionLimiter, MessageValidator, RateLimiter};
use crate::moderation::{Moderation, Punishment};
use crate::store::{FindUser, Persist, State, Store, Write};
use log::*;

use actix::*;
use actix_web::{web, HttpRequest, HttpResponse};
use rand::{rngs::SysRng, SeedableRng};
use rand_hc::Hc128Rng;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use uuid::Uuid;

const MAX_FRAME_SIZE: usize = 64 * 1024;
/// Outgoing login requests at once; a restart reconnects everyone at the same time.
const CONCURRENT_LOGINS: usize = 64;
const ROLE_REFRESH: Duration = Duration::from_secs(600);
const MAINTENANCE: Duration = Duration::from_secs(3600);
const HISTORY: usize = 1000;
const PARTY_TICK: Duration = Duration::from_secs(2);

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
    /// Everyone seen since startup or related to someone.
    directory: HashMap<UserId, Identity>,

    store: Addr<Store>,
    api: Arc<Api>,
    logins: Arc<Semaphore>,
    /// In the order of the Service API, which is the order prefixes are shown in.
    roles: Vec<RoleDefinition>,
    social: Social,
    groups: Groups,
    parties: Parties,
    party_snapshots: HashMap<UserId, String>,
    member_states: HashMap<UserId, MemberState>,
    /// Servers whose hashed seeds are hidden or randomized.
    unreliable_seeds: HashSet<String>,
    rng: Hc128Rng,
    validator: MessageValidator,
    moderation: Moderation,
    config: Config,

    current_internal_user_id: u64,
    history: VecDeque<Recorded>,
    recent_reports: Vec<RecentReport>,
    next_message_id: u64,
}

impl ChatServer {
    pub fn new(
        config: Config,
        store: Addr<Store>,
        punishments: Vec<Punishment>,
        state: State,
    ) -> ChatServer {
        ChatServer {
            connections: HashMap::new(),
            users: HashMap::new(),
            directory: state.users.iter().map(|model| (model.id, Identity::of(model))).collect(),

            store,
            api: Arc::new(Api::new(&config.api)),
            logins: Arc::new(Semaphore::new(CONCURRENT_LOGINS)),
            roles: Vec::new(),
            social: Social::new(state.relations),
            groups: Groups::new(state.groups, state.members),
            parties: Parties::default(),
            party_snapshots: HashMap::new(),
            member_states: HashMap::new(),
            unreliable_seeds: HashSet::new(),
            rng: Hc128Rng::try_from_rng(&mut SysRng).expect("could not initialize hc128 rng"),
            validator: MessageValidator::new(config.message.clone()),
            moderation: Moderation::new(punishments),
            config,

            current_internal_user_id: 0,
            history: VecDeque::with_capacity(HISTORY),
            recent_reports: Vec::new(),
            next_message_id: 1,
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

    /// Resolves the name of an online user: exact spelling before case-insensitive,
    /// Minecraft accounts before LiquidBounce Accounts.
    fn find_online(&self, name: &str, scope: Scope) -> Option<UserId> {
        self.users
            .keys()
            .filter_map(|id| self.directory.get(id))
            .filter(|identity| identity.name.eq_ignore_ascii_case(name) && scope.includes(identity.kind))
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

    fn send_v2(&self, id: InternalId, packet: ClientPacket) -> bool {
        self.connections
            .get(&id)
            .is_some_and(|connection| connection.protocol >= Protocol::V2 && send_message(connection, packet))
    }

    fn persist(&self, writes: Vec<Write>) {
        self.store.do_send(Persist(writes));
    }

    /// Resolves a public id or name: online users directly, everyone else from the database.
    fn resolve_user<F>(&mut self, ctx: &mut Context<Self>, query: String, scope: Scope, then: F)
    where
        F: FnOnce(&mut ChatServer, &mut Context<ChatServer>, Option<Resolved>) + 'static,
    {
        let online = Uuid::parse_str(&query)
            .ok()
            .filter(|id| self.users.contains_key(id) && scope.includes(self.identity(*id).kind))
            .or_else(|| self.find_online(&query, scope));
        if let Some(user) = online {
            let resolved = Resolved {
                identity: self.identity(user).clone(),
                accept_friend_requests: self.users[&user].accept_friend_requests,
                last_ip: self.online_connections(user).next().map(|(_, connection)| connection.ip),
            };
            then(self, ctx, Some(resolved));
            return;
        }

        let store = self.store.clone();
        let accounts = scope == Scope::Accounts;
        ctx.spawn(async move { store.send(FindUser { query, accounts }).await }.into_actor(self).map(
            move |result, actor, ctx| {
                let resolved = match result {
                    Ok(Ok(model)) => model.map(|model| {
                        let identity = Identity::of(&model);
                        actor.directory.entry(identity.id).or_insert_with(|| identity.clone());
                        Resolved {
                            identity,
                            accept_friend_requests: model.accept_friend_requests,
                            last_ip: model.last_ip.and_then(|ip| ip.parse().ok()),
                        }
                    }),
                    Ok(Err(err)) => {
                        error!("Could not look up user: {}", err);
                        None
                    }
                    Err(err) => {
                        error!("Store unavailable: {}", err);
                        None
                    }
                };
                then(actor, ctx, resolved)
            },
        ));
    }

    /// The socket stays open, as old clients reconnect at once.
    fn logout(&mut self, id: InternalId) {
        let Some(connection) = self.connections.get_mut(&id) else { return };
        let Login::User(user) = connection.login else { return };
        connection.login = Login::Anonymous;
        connection.location = None;
        let Some(online) = self.users.get_mut(&user) else { return };
        online.connections.retain(|connection| *connection != id);
        if online.game == Some(id) {
            online.game = online
                .connections
                .iter()
                .copied()
                .find(|other| self.connections.get(other).is_some_and(|other| other.location.is_some()));
        }
        if online.connections.is_empty() {
            self.users.remove(&user);
            self.parties.set_online(user, false, now_ms());
            self.send_presence(user);
        }
        self.refresh_party(user);
    }

    fn game_location(&self, user: UserId) -> Option<&Location> {
        let game = self.users.get(&user)?.game?;
        self.connections.get(&game)?.location.as_ref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Scope {
    /// Messages, friends, parties and groups are between LiquidBounce Accounts.
    Accounts,
    Anyone,
}

impl Scope {
    fn includes(self, kind: Kind) -> bool {
        self == Scope::Anyone || kind == Kind::Account
    }
}

pub(super) struct Resolved {
    identity: Identity,
    accept_friend_requests: bool,
    last_ip: Option<IpAddr>,
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
        ctx.run_interval(PARTY_TICK, |actor, _ctx| actor.party_tick());
        ctx.run_interval(MAINTENANCE, |actor, _ctx| {
            let now = now_ms();
            actor.moderation.prune(now);
            let retention = actor.config.moderation.ip_retention.as_millis() as i64;
            actor.persist(vec![Write::ForgetIps { seen_before: now - retention }]);
        });
    }
}

impl ChatServer {
    fn refresh_roles(&mut self, ctx: &mut Context<Self>) {
        let api = self.api.clone();
        ctx.spawn(async move { api.roles().await }.into_actor(self).map(|result, actor, _ctx| {
            match result {
                Ok(roles) => {
                    actor.roles = roles;
                }
                Err(err) => warn!("Could not refresh roles: {}", err),
            }
        }));
    }

    fn is_staff(&self, user: UserId) -> bool {
        self.users
            .get(&user)
            .is_some_and(|online| self.has_staff_role(&online.roles))
    }

    fn has_perks(&self, user: UserId) -> bool {
        self.is_staff(user)
            || self
                .users
                .get(&user)
                .is_some_and(|online| online.roles.iter().any(|role| self.config.message.perk_roles.contains(role)))
    }

    fn has_staff_role(&self, roles: &[String]) -> bool {
        roles
            .iter()
            .any(|role| self.role(role).is_some_and(|role| role.is_staff))
    }

    fn role(&self, id: &str) -> Option<&RoleDefinition> {
        self.roles.iter().find(|role| role.id == id)
    }

    fn user_ref(&self, user: UserId) -> UserRef {
        self.known_ref(user).expect("online users are in the directory")
    }

    /// The head follows the Minecraft account they play on.
    fn known_ref(&self, user: UserId) -> Option<UserRef> {
        let mut reference = UserRef::from(self.directory.get(&user)?);
        reference.minecraft = self
            .online_connections(user)
            .find_map(|(_, connection)| connection.minecraft.clone());
        if let Some(minecraft) = &reference.minecraft {
            reference.uuid = minecraft.uuid;
        }
        Some(reference)
    }

    fn author(&self, user: UserId) -> Author {
        let mut roles: Vec<RoleView> = self
            .users
            .get(&user)
            .into_iter()
            .flat_map(|online| online.roles.iter())
            .map(|id| match self.role(id) {
                Some(role) => RoleView {
                    id: role.id.clone(),
                    name: role.display_name.clone(),
                    staff: role.is_staff,
                },
                None => RoleView {
                    id: id.clone(),
                    name: id.clone(),
                    staff: false,
                },
            })
            .collect();
        let position = |role: &RoleView| self.roles.iter().position(|known| known.id == role.id).unwrap_or(usize::MAX);
        roles.sort_by_key(|role| (!role.staff, position(role)));

        Author {
            user: self.user_ref(user),
            roles,
            highlight: self.has_perks(user),
        }
    }
}

impl Handler<Disconnect> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Disconnect, _ctx: &mut Context<Self>) {
        info!("User `{}` disconnected.", msg.id);
        self.logout(msg.id);
        self.connections.remove(&msg.id);
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
    server_chat: bool,
    minecraft: Option<Player>,
    location: Option<Location>,
    limits: StateLimits,
}

struct StateLimits {
    location: ActionLimiter,
    sightings: ActionLimiter,
    position: ActionLimiter,
    status: ActionLimiter,
    inventory: ActionLimiter,
}

impl Default for StateLimits {
    fn default() -> StateLimits {
        StateLimits {
            location: ActionLimiter::new(5, 2.0),
            sightings: ActionLimiter::new(5, 2.0),
            position: ActionLimiter::new(10, 10.0),
            status: ActionLimiter::new(4, 4.0),
            inventory: ActionLimiter::new(2, 1.0),
        }
    }
}

#[derive(Default)]
struct MemberState {
    position: Option<Position>,
    status: Option<serde_json::Value>,
    inventory: Option<serde_json::Value>,
}

impl Connection {
    fn user(&self) -> Option<UserId> {
        match self.login {
            Login::User(user) => Some(user),
            _ => None,
        }
    }
}

struct OnlineUser {
    connections: Vec<InternalId>,
    rate_limiter: RateLimiter,
    actions: ActionLimiter,
    roles: Vec<String>,
    hide_server: bool,
    accept_friend_requests: bool,
    created_at: i64,
    game: Option<InternalId>,
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

struct Recorded {
    id: u64,
    channel: Channel,
    author: UserId,
    content: String,
    time: i64,
    /// `None` for public channels.
    audience: Option<Vec<UserId>>,
}

struct RecentReport {
    at: i64,
    target: UserId,
    reporter: UserId,
    message: Option<u64>,
    /// The reporter's network, if the report counts towards an automatic mute.
    network: Option<crate::ip::Cidr>,
}
