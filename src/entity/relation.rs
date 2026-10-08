use sea_orm::entity::prelude::*;

/// A friendship is two rows, a request or block one, from `user_id` towards `target_id`.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "relations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false, indexed)]
    pub target_id: Uuid,
    pub kind: Kind,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(8))")]
pub enum Kind {
    #[sea_orm(string_value = "friend")]
    Friend,
    #[sea_orm(string_value = "request")]
    Request,
    #[sea_orm(string_value = "block")]
    Block,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
