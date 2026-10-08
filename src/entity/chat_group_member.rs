use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "chat_group_members")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub group_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false, indexed)]
    pub user_id: Uuid,
    pub role: Role,
    pub joined_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(8))")]
#[serde(rename_all = "lowercase")]
pub enum Role {
    #[sea_orm(string_value = "owner")]
    Owner,
    #[sea_orm(string_value = "admin")]
    Admin,
    #[sea_orm(string_value = "member")]
    Member,
    #[sea_orm(string_value = "invited")]
    Invited,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
