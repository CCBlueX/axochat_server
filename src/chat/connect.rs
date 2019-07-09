use log::*;

use super::{ChatServer, ClientPacket, InternalId, SessionState};
use actix::*;

#[derive(Message)]
#[rtype(InternalId)]
pub(super) struct Connect {
    addr: Recipient<ClientPacket>,
}

impl Connect {
    pub fn new(addr: Recipient<ClientPacket>) -> Connect {
        Connect { addr }
    }
}

impl Handler<Connect> for ChatServer {
    type Result = InternalId;

    fn handle(&mut self, msg: Connect, _ctx: &mut Context<Self>) -> InternalId {
        self.current_internal_user_id += 1;
        let id = InternalId::new(self.current_internal_user_id);
        self.connections.insert(
            id,
            SessionState {
                addr: msg.addr.clone(),
                session_hash: None,
                user: None,
            },
        );
        debug!("User `{}` joined the chat.", id);
        id
    }
}
