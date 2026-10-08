use crate::chat::{new_id, now_ms, ChatServer, ClientPacket, Identity, InternalId, Login, OnlineUser, SuccessReason};
use crate::entity::user;
use crate::error::ClientError;
use crate::message::RateLimiter;
use crate::store::{Identify, IdentityKey, Store};
use log::*;

use actix::*;
use std::net::IpAddr;
use uuid::Uuid;

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

        let verified = match result {
            Ok(verified) => verified,
            Err(error) => {
                connection.login = Login::Anonymous;
                self.send_error(id, error);
                return;
            }
        };

        let identity = Identity::of(&verified.model);
        info!("User `{}` logged in as `{}` ({}).", id, identity.name, identity.id);
        connection.login = Login::User(identity.id);
        connection.allow_messages = allow_messages;
        connection.session_hash = None;

        let message_config = &self.config.message;
        let online = self.users.entry(identity.id).or_insert_with(|| OnlineUser {
            connections: Vec::new(),
            rate_limiter: RateLimiter::new(message_config.clone()),
            roles: Vec::new(),
        });
        online.connections.push(id);
        online.roles = verified.roles;
        self.directory.insert(identity.id, identity);

        self.send(id, ClientPacket::Success { reason: SuccessReason::Login });
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
