use actix::dev::MessageResponse;
use actix::*;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Serialize, Deserialize, Copy, Clone, Eq, PartialEq, Hash)]
#[serde(transparent)]
pub struct InternalId(u64);

impl InternalId {
    pub fn new(id: u64) -> InternalId {
        InternalId(id)
    }
}

impl fmt::Display for InternalId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}

impl<A, M> MessageResponse<A, M> for InternalId
where
    A: Actor,
    M: Message<Result = InternalId>,
{
    fn handle(self, _: &mut A::Context, tx: Option<actix::dev::OneshotSender<InternalId>>) {
        if let Some(tx) = tx {
            let _ = tx.send(self);
        }
    }
}

/// A UUIDv7: ids sort by creation time, which keeps the database indices compact.
pub fn new_id(rng: &mut impl rand::Rng) -> uuid::Uuid {
    let mut bytes = [0; 10];
    rng.fill_bytes(&mut bytes);
    uuid::Builder::from_unix_timestamp_millis(now_ms() as u64, &bytes).into_uuid()
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time is somehow before the unix epoch")
        .as_millis() as i64
}
