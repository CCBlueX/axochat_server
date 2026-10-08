use log::*;

use super::{session::Frame, ChatServer, InternalId, Protocol, SessionState};
use actix::*;
use std::net::IpAddr;

#[derive(Message)]
#[rtype(InternalId)]
pub(super) struct Connect {
    addr: Recipient<Frame>,
    ip: IpAddr,
}

impl Connect {
    pub fn new(addr: Recipient<Frame>, ip: IpAddr) -> Connect {
        Connect { addr, ip }
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
                addr: msg.addr,
                ip: msg.ip,
                protocol: Protocol::V1,
                session_hash: None,
                login_pending: false,
                user: None,
            },
        );
        debug!("User `{}` joined the chat from {}.", id, msg.ip);
        id
    }
}
