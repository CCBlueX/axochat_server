use crate::chat::{new_id, now_ms, ChatServer, ClientPacket, Identity, InternalId, Login, OnlineUser, SuccessReason};
use crate::entity::{punishment::Kind, user};
use crate::error::ClientError;
use crate::message::{ActionLimiter, RateLimiter};
use crate::store::{Identify, IdentityKey, Store};
use log::*;

use actix::*;
use std::net::IpAddr;
use uuid::Uuid;

const ACTION_BURST: u32 = 20;
const ACTION_RATE: f64 = 2.0;

pub(super) struct Verified {
    pub model: user::Model,
    pub roles: Vec<String>,
}

/// Taken before the login goes async.
pub(super) struct Visit {
    ip: IpAddr,
    id: Uuid,
    at: i64,
}

impl Visit {
    pub fn identify(self, key: IdentityKey, name: String, linked: Option<Uuid>) -> Identify {
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
        let connection = self.connections.get_mut(&id)?;
        if connection.login != Login::Anonymous {
            info!("User `{}` tried to log in multiple times.", id);
            self.send_error(id, ClientError::AlreadyLoggedIn);
            return None;
        }
        connection.login = Login::Pending;
        Some(Visit {
            ip: connection.ip,
            id: new_id(&mut self.rng),
            at: now_ms(),
        })
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
        self.send(id, ClientPacket::Success { reason: SuccessReason::Login });

        if came_online {
            self.send_presence(user);
        }
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
