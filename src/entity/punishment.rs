use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "punishments")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub kind: Kind,
    #[sea_orm(indexed, nullable)]
    pub user_id: Option<Uuid>,
    #[sea_orm(indexed, column_type = "String(StringLen::N(49))", nullable)]
    pub ip: Option<String>,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub reason: String,
    #[sea_orm(nullable)]
    pub issued_by: Option<Uuid>,
    pub created_at: i64,
    #[sea_orm(nullable)]
    pub expires_at: Option<i64>,
    #[sea_orm(indexed, nullable)]
    pub revoked_at: Option<i64>,
    #[sea_orm(nullable)]
    pub revoked_by: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(8))")]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[sea_orm(string_value = "mute")]
    Mute,
    #[sea_orm(string_value = "ban")]
    Ban,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
