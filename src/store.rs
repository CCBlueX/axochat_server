use crate::entity::user;

use actix::prelude::*;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, Set};
use std::net::IpAddr;
use uuid::Uuid;

/// Serializes all database access: every message finishes before the next one starts,
/// so a read always sees the writes queued before it.
pub struct Store {
    db: DatabaseConnection,
}

impl Store {
    pub fn new(db: DatabaseConnection) -> Store {
        Store { db }
    }
}

impl Actor for Store {
    type Context = Context<Self>;
}

macro_rules! atomic {
    ($actor:expr, $db:ident => $body:expr) => {{
        let $db = $actor.db.clone();
        AtomicResponse::new(Box::pin(async move { $body }.into_actor($actor)))
    }};
}

pub enum IdentityKey {
    Account(String),
    Minecraft(Uuid),
}

/// Finds or creates the user behind a login and records the visit.
#[derive(Message)]
#[rtype(result = "Result<user::Model, DbErr>")]
pub struct Identify {
    pub key: IdentityKey,
    pub name: String,
    pub linked: Option<Uuid>,
    pub ip: IpAddr,
    /// The id of the user if it is new.
    pub id: Uuid,
    pub at: i64,
}

impl Handler<Identify> for Store {
    type Result = AtomicResponse<Self, Result<user::Model, DbErr>>;

    fn handle(&mut self, msg: Identify, _ctx: &mut Context<Self>) -> Self::Result {
        atomic!(self, db => {
            let existing = match &msg.key {
                IdentityKey::Account(account) => {
                    user::Entity::find().filter(user::Column::Account.eq(account.as_str())).one(&db).await?
                }
                IdentityKey::Minecraft(uuid) => {
                    user::Entity::find().filter(user::Column::MinecraftUuid.eq(*uuid)).one(&db).await?
                }
            };

            match existing {
                Some(model) => {
                    let mut model: user::ActiveModel = model.into();
                    model.name = Set(msg.name);
                    model.last_seen_at = Set(msg.at);
                    model.last_ip = Set(Some(msg.ip.to_string()));
                    if let IdentityKey::Account(_) = msg.key {
                        model.linked_minecraft_uuid = Set(msg.linked);
                    }
                    model.update(&db).await
                }
                None => {
                    let (account, minecraft_uuid) = match msg.key {
                        IdentityKey::Account(account) => (Some(account), None),
                        IdentityKey::Minecraft(uuid) => (None, Some(uuid)),
                    };
                    user::ActiveModel {
                        id: Set(msg.id),
                        account: Set(account),
                        minecraft_uuid: Set(minecraft_uuid),
                        linked_minecraft_uuid: Set(msg.linked),
                        name: Set(msg.name),
                        hide_server: Set(false),
                        accept_friend_requests: Set(true),
                        created_at: Set(msg.at),
                        last_seen_at: Set(msg.at),
                        last_ip: Set(Some(msg.ip.to_string())),
                    }
                    .insert(&db)
                    .await
                }
            }
        })
    }
}
