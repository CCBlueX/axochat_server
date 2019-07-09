use actix::{
    dev::{MessageResponse, ResponseChannel},
    *,
};
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
    fn handle<R: ResponseChannel<M>>(self, _: &mut A::Context, tx: Option<R>) {
        if let Some(tx) = tx {
            tx.send(self);
        }
    }
}
