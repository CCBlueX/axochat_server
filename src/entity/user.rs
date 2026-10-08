use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique, column_type = "String(StringLen::N(64))", nullable)]
    pub account: Option<String>,
    #[sea_orm(unique, nullable)]
    pub minecraft_uuid: Option<Uuid>,
    #[sea_orm(indexed, nullable)]
    pub linked_minecraft_uuid: Option<Uuid>,
    #[sea_orm(indexed, column_type = "String(StringLen::N(64))")]
    pub name: String,
    #[sea_orm(column_type = "TinyInteger")]
    pub hide_server: bool,
    #[sea_orm(column_type = "TinyInteger")]
    pub accept_friend_requests: bool,
    pub created_at: i64,
    #[sea_orm(indexed)]
    pub last_seen_at: i64,
    #[sea_orm(column_type = "String(StringLen::N(45))", nullable)]
    pub last_ip: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
