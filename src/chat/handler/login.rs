use crate::chat::world::Player;
use crate::chat::{
    new_id, now_ms, ChatServer, ClientPacket, Identity, InternalId, Login, OnlineUser, SuccessReason, LOGIN_BURST,
    LOGIN_MAX_WAIT, LOGIN_RATE,
};
use crate::entity::{punishment::Kind, user};
use crate::error::ClientError;
use crate::message::{ActionLimiter, RateLimiter};
use crate::store::{Identify, IdentityKey, Store};
use log::*;

use actix::*;
use std::net::IpAddr;
use std::time::Duration;
use uuid::Uuid;

const ACTION_BURST: u32 = 20;
const ACTION_RATE: f64 = 2.0;
const MAX_NAME: usize = 32;

pub(super) struct Verified {
    pub model: user::Model,
    pub roles: Vec<String>,
    pub minecraft: Option<Player>,
}

/// Taken before the login goes async.
pub(super) struct Visit {
    ip: IpAddr,
    id: Uuid,
    at: i64,
    pub delay: Duration,
}

impl Visit {
    pub fn identify(self, key: IdentityKey, name: String, linked: Option<Uuid>) -> Identify {
        let name = match &key {
            IdentityKey::Account(account) => account_name(&name, account),
            IdentityKey::Minecraft(_) => name,
        };
        Identify {
            key,
            name: name.chars().take(64).collect(),
            linked,
            ip: self.ip,
            id: self.id,
            at: self.at,
        }
    }
}

impl ChatServer {
    pub(super) fn begin_login(&mut self, id: InternalId) -> Option<Visit> {
        let connection = self.connections.get(&id)?;
        if connection.login != Login::Anonymous {
            info!("User `{}` tried to log in multiple times.", id);
            self.send_error(id, ClientError::AlreadyLoggedIn);
            return None;
        }
        let delay = self.login_turn(id)?;
        let connection = self.connections.get_mut(&id)?;
        connection.login = Login::Pending;
        Some(Visit {
            ip: connection.ip,
            id: new_id(&mut self.rng),
            at: now_ms(),
            delay,
        })
    }

    /// When a connection may ask Mojang or the Service API, so no address can make the server flood them.
    pub(super) fn login_turn(&mut self, id: InternalId) -> Option<Duration> {
        let ip = self.connections.get(&id)?.ip;
        let pace = self
            .login_pace
            .entry(ip)
            .or_insert_with(|| ActionLimiter::new(LOGIN_BURST, LOGIN_RATE));
        let turn = pace.reserve(LOGIN_MAX_WAIT);
        if turn.is_none() {
            info!("Logins from {} are rate limited.", ip);
            self.send_error(id, ClientError::RateLimited);
        }
        turn
    }

    pub(super) fn finish_login(&mut self, id: InternalId, result: Result<Verified, ClientError>, allow_messages: bool) {
        let Some(connection) = self.connections.get_mut(&id) else { return };
        if connection.login != Login::Pending {
            return;
        }
        connection.login = Login::Anonymous;
        let ip = connection.ip;

        let verified = match result {
            Ok(verified) => verified,
            Err(error) => {
                self.send_error(id, error);
                return;
            }
        };

        let identity = Identity::of(&verified.model);
        let ban = self
            .moderation
            .find(Kind::Ban, Some(identity.id), Some(ip), now_ms())
            .filter(|_| !self.has_staff_role(&verified.roles));
        if let Some(ban) = ban {
            info!("Banned user `{}` ({}) tried to log in.", identity.name, identity.id);
            let punished = ClientPacket::Punished {
                kind: ban.kind,
                reason: ban.reason.clone(),
                expires: ban.expires_at,
            };
            self.send_v2(id, punished);
            self.send_error(id, ClientError::Banned);
            return;
        }

        info!("User `{}` logged in as `{}` ({}).", id, identity.name, identity.id);
        let Some(connection) = self.connections.get_mut(&id) else { return };
        connection.login = Login::User(identity.id);
        connection.allow_messages = allow_messages;
        connection.session_hash = None;
        connection.minecraft = verified.minecraft;

        let user = identity.id;
        let came_online = !self.users.contains_key(&user);
        let message_config = &self.config.message;
        let online = self.users.entry(user).or_insert_with(|| OnlineUser {
            connections: Vec::new(),
            rate_limiter: RateLimiter::new(message_config.clone()),
            actions: ActionLimiter::new(ACTION_BURST, ACTION_RATE),
            roles: Vec::new(),
            hide_server: verified.model.hide_server,
            accept_friend_requests: verified.model.accept_friend_requests,
            created_at: verified.model.created_at,
            game: None,
        });
        online.connections.push(id);
        online.roles = verified.roles;
        self.directory.insert(user, identity);

        let welcome = ClientPacket::Welcome {
            user: self.author(user),
            staff: self.is_staff(user),
        };
        self.send_v2(id, welcome);
        if let Some(settings) = self.settings(id) {
            self.send_v2(id, ClientPacket::Settings(settings));
        }
        self.send_friends(user);
        self.send_blocks(user);
        self.send_groups(user);
        self.party_welcome(user);
        self.send(id, ClientPacket::Success { reason: SuccessReason::Login });

        if came_online {
            self.send_presence(user);
        }
        self.party_login(user);
    }
}

pub(super) async fn identify(store: Addr<Store>, request: Identify) -> Result<user::Model, ClientError> {
    match store.send(request).await {
        Ok(Ok(model)) => Ok(model),
        Ok(Err(err)) => {
            error!("Could not store user: {}", err);
            Err(ClientError::Internal)
        }
        Err(err) => {
            error!("Store unavailable: {}", err);
            Err(ClientError::Internal)
        }
    }
}

/// Nicknames are free text: formatting codes, invisible and direction-changing characters go, and
/// whitespace becomes `_` so commands can address the result.
fn account_name(nickname: &str, account: &str) -> String {
    let mut name = String::new();
    let mut gap = false;
    for ch in nickname.chars() {
        if ch.is_whitespace() {
            gap = !name.is_empty();
        } else if ch != '§' && (ch.is_ascii_graphic() || ch.is_alphanumeric()) {
            if gap {
                name.push('_');
                gap = false;
            }
            name.push(ch);
        }
    }
    let name: String = name.chars().take(MAX_NAME).collect();
    if name.is_empty() {
        format!("user-{}", account.chars().take(8).collect::<String>())
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::account_name;

    #[test]
    fn account_names() {
        assert_eq!(account_name("  Izuna  Seikatsu ", "a"), "Izuna_Seikatsu");
        assert_eq!(account_name("§cStaff", "a"), "cStaff");
        assert_eq!(account_name("a\u{202e}b\u{200b}c\nd", "a"), "abc_d");
        assert_eq!(account_name("ünïcödé", "a"), "ünïcödé");
        assert_eq!(account_name("\u{200b}", "0123456789"), "user-01234567");
        assert_eq!(account_name(&"x".repeat(40), "a").len(), 32);
    }
}
