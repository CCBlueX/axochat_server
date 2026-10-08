use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "reports")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(indexed, unique_key = "report")]
    pub reporter_id: Uuid,
    #[sea_orm(indexed, unique_key = "report")]
    pub target_id: Uuid,
    #[sea_orm(column_type = "String(StringLen::N(48))", nullable)]
    pub channel: Option<String>,
    #[sea_orm(nullable, unique_key = "report")]
    pub message_id: Option<i64>,
    #[sea_orm(column_type = "String(StringLen::N(512))", nullable)]
    pub content: Option<String>,
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub reason: String,
    pub created_at: i64,
    #[sea_orm(nullable)]
    pub resolved_by: Option<Uuid>,
    #[sea_orm(indexed, nullable)]
    pub resolved_at: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
