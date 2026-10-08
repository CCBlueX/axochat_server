use crate::entity::{chat_group, chat_group_member, punishment, relation, user};
use log::*;

use actix::prelude::*;
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder, Set,
    TransactionTrait,
};
use std::net::IpAddr;
use std::time::Duration;
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

/// Changes made in memory first, written in order in one transaction.
#[derive(Message)]
#[rtype(result = "()")]
pub struct Persist(pub Vec<Write>);

#[derive(Debug, Clone, PartialEq)]
pub enum Write {
    Punish(punishment::Model),
    Revoke { ids: Vec<Uuid>, by: Option<Uuid>, at: i64 },
    ForgetIps { seen_before: i64 },
    SetRelation { user: Uuid, target: Uuid, kind: relation::Kind, at: i64 },
    RemoveRelation { user: Uuid, target: Uuid },
    Settings { user: Uuid, hide_server: bool, accept_friend_requests: bool },
    CreateGroup { id: Uuid, name: String, owner: Uuid, at: i64 },
    RenameGroup { id: Uuid, name: String },
    GroupOwner { id: Uuid, owner: Uuid },
    DeleteGroup { id: Uuid },
    GroupMember { group: Uuid, user: Uuid, role: chat_group_member::Role, at: i64 },
    RemoveGroupMember { group: Uuid, user: Uuid },
}

const ATTEMPTS: u32 = 3;

impl Handler<Persist> for Store {
    type Result = AtomicResponse<Self, ()>;

    fn handle(&mut self, Persist(writes): Persist, _ctx: &mut Context<Self>) -> Self::Result {
        atomic!(self, db => {
            for attempt in 1..=ATTEMPTS {
                match apply(&db, &writes).await {
                    Ok(()) => return,
                    Err(err) if attempt < ATTEMPTS => {
                        warn!("Could not write {:?}, retrying: {}", writes, err);
                        actix::clock::sleep(Duration::from_secs(1 << attempt)).await;
                    }
                    Err(err) => error!("Could not write {:?}: {}", writes, err),
                }
            }
        })
    }
}

async fn apply(db: &DatabaseConnection, writes: &[Write]) -> Result<(), DbErr> {
    let txn = db.begin().await?;
    for write in writes {
        match write.clone() {
            Write::Punish(model) => {
                punishment::ActiveModel::from(model).insert(&txn).await?;
            }
            Write::Revoke { ids, by, at } => {
                punishment::Entity::update_many()
                    .col_expr(punishment::Column::RevokedAt, Expr::value(at))
                    .col_expr(punishment::Column::RevokedBy, Expr::value(by))
                    .filter(punishment::Column::Id.is_in(ids))
                    .exec(&txn)
                    .await?;
            }
            Write::ForgetIps { seen_before } => {
                user::Entity::update_many()
                    .col_expr(user::Column::LastIp, Expr::value(Option::<String>::None))
                    .filter(user::Column::LastSeenAt.lt(seen_before))
                    .filter(user::Column::LastIp.is_not_null())
                    .exec(&txn)
                    .await?;
            }
            Write::SetRelation { user, target, kind, at } => {
                relation::Entity::insert(relation::ActiveModel {
                    user_id: Set(user),
                    target_id: Set(target),
                    kind: Set(kind),
                    created_at: Set(at),
                })
                .on_conflict(
                    OnConflict::columns([relation::Column::UserId, relation::Column::TargetId])
                        .update_columns([relation::Column::Kind, relation::Column::CreatedAt])
                        .to_owned(),
                )
                .exec(&txn)
                .await?;
            }
            Write::RemoveRelation { user, target } => {
                relation::Entity::delete_by_id((user, target)).exec(&txn).await?;
            }
            Write::Settings { user, hide_server, accept_friend_requests } => {
                user::Entity::update_many()
                    .col_expr(user::Column::HideServer, Expr::value(hide_server))
                    .col_expr(user::Column::AcceptFriendRequests, Expr::value(accept_friend_requests))
                    .filter(user::Column::Id.eq(user))
                    .exec(&txn)
                    .await?;
            }
            Write::CreateGroup { id, name, owner, at } => {
                chat_group::ActiveModel {
                    id: Set(id),
                    name: Set(name),
                    owner_id: Set(owner),
                    created_at: Set(at),
                }
                .insert(&txn)
                .await?;
            }
            Write::RenameGroup { id, name } => {
                chat_group::Entity::update_many()
                    .col_expr(chat_group::Column::Name, Expr::value(name))
                    .filter(chat_group::Column::Id.eq(id))
                    .exec(&txn)
                    .await?;
            }
            Write::GroupOwner { id, owner } => {
                chat_group::Entity::update_many()
                    .col_expr(chat_group::Column::OwnerId, Expr::value(owner))
                    .filter(chat_group::Column::Id.eq(id))
                    .exec(&txn)
                    .await?;
            }
            Write::DeleteGroup { id } => {
                chat_group_member::Entity::delete_many()
                    .filter(chat_group_member::Column::GroupId.eq(id))
                    .exec(&txn)
                    .await?;
                chat_group::Entity::delete_by_id(id).exec(&txn).await?;
            }
            Write::GroupMember { group, user, role, at } => {
                chat_group_member::Entity::insert(chat_group_member::ActiveModel {
                    group_id: Set(group),
                    user_id: Set(user),
                    role: Set(role),
                    joined_at: Set(at),
                })
                .on_conflict(
                    OnConflict::columns([chat_group_member::Column::GroupId, chat_group_member::Column::UserId])
                        .update_columns([chat_group_member::Column::Role, chat_group_member::Column::JoinedAt])
                        .to_owned(),
                )
                .exec(&txn)
                .await?;
            }
            Write::RemoveGroupMember { group, user } => {
                chat_group_member::Entity::delete_by_id((group, user)).exec(&txn).await?;
            }
        }
    }
    txn.commit().await
}

pub struct State {
    pub relations: Vec<relation::Model>,
    pub groups: Vec<chat_group::Model>,
    pub members: Vec<chat_group_member::Model>,
    pub users: Vec<user::Model>,
}

#[derive(Message)]
#[rtype(result = "Result<State, DbErr>")]
pub struct LoadState;

impl Handler<LoadState> for Store {
    type Result = AtomicResponse<Self, Result<State, DbErr>>;

    fn handle(&mut self, _msg: LoadState, _ctx: &mut Context<Self>) -> Self::Result {
        atomic!(self, db => {
            let relations = relation::Entity::find().all(&db).await?;
            let groups = chat_group::Entity::find().all(&db).await?;
            let members = chat_group_member::Entity::find().all(&db).await?;

            let mut ids: Vec<Uuid> = relations
                .iter()
                .flat_map(|r| [r.user_id, r.target_id])
                .chain(members.iter().map(|m| m.user_id))
                .collect();
            ids.sort_unstable();
            ids.dedup();
            let mut users = Vec::with_capacity(ids.len());
            for chunk in ids.chunks(1000) {
                users.extend(user::Entity::find().filter(user::Column::Id.is_in(chunk.to_vec())).all(&db).await?);
            }
            Ok(State { relations, groups, members, users })
        })
    }
}

#[derive(Message)]
#[rtype(result = "Result<Vec<punishment::Model>, DbErr>")]
pub struct LoadPunishments {
    pub now: i64,
}

impl Handler<LoadPunishments> for Store {
    type Result = AtomicResponse<Self, Result<Vec<punishment::Model>, DbErr>>;

    fn handle(&mut self, msg: LoadPunishments, _ctx: &mut Context<Self>) -> Self::Result {
        atomic!(self, db => {
            punishment::Entity::find()
                .filter(punishment::Column::RevokedAt.is_null())
                .filter(
                    Condition::any()
                        .add(punishment::Column::ExpiresAt.is_null())
                        .add(punishment::Column::ExpiresAt.gt(msg.now)),
                )
                .all(&db)
                .await
        })
    }
}

/// A user by public id or name; of several with the name, the one seen last.
#[derive(Message)]
#[rtype(result = "Result<Option<user::Model>, DbErr>")]
pub struct FindUser(pub String);

impl Handler<FindUser> for Store {
    type Result = AtomicResponse<Self, Result<Option<user::Model>, DbErr>>;

    fn handle(&mut self, FindUser(query): FindUser, _ctx: &mut Context<Self>) -> Self::Result {
        atomic!(self, db => {
            if let Ok(id) = Uuid::parse_str(&query) {
                if let Some(user) = user::Entity::find_by_id(id).one(&db).await? {
                    return Ok(Some(user));
                }
            }
            user::Entity::find()
                .filter(user::Column::Name.eq(query))
                .order_by_desc(user::Column::LastSeenAt)
                .one(&db)
                .await
        })
    }
}

/// The users a v1 ban by Minecraft UUID addresses: the Minecraft user, or accounts linked to it.
/// Bans of unknown UUIDs create the Minecraft user, as banning unseen players always worked.
#[derive(Message)]
#[rtype(result = "Result<Vec<user::Model>, DbErr>")]
pub struct MinecraftTargets {
    pub uuid: Uuid,
    pub create: Option<(Uuid, i64)>,
}

impl Handler<MinecraftTargets> for Store {
    type Result = AtomicResponse<Self, Result<Vec<user::Model>, DbErr>>;

    fn handle(&mut self, msg: MinecraftTargets, _ctx: &mut Context<Self>) -> Self::Result {
        atomic!(self, db => {
            let users = user::Entity::find()
                .filter(
                    Condition::any()
                        .add(user::Column::MinecraftUuid.eq(msg.uuid))
                        .add(user::Column::LinkedMinecraftUuid.eq(msg.uuid)),
                )
                .all(&db)
                .await?;
            match msg.create {
                Some((id, at)) if users.is_empty() => {
                    let user = user::ActiveModel {
                        id: Set(id),
                        account: Set(None),
                        minecraft_uuid: Set(Some(msg.uuid)),
                        linked_minecraft_uuid: Set(None),
                        name: Set(msg.uuid.hyphenated().to_string()),
                        hide_server: Set(false),
                        accept_friend_requests: Set(true),
                        created_at: Set(at),
                        last_seen_at: Set(at),
                        last_ip: Set(None),
                    }
                    .insert(&db)
                    .await?;
                    Ok(vec![user])
                }
                _ => Ok(users),
            }
        })
    }
}
