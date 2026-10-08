use log::*;

use super::{session::Frame, ChatServer, Connection, InternalId, Login, Protocol};
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
            Connection {
                addr: msg.addr,
                ip: msg.ip,
                protocol: Protocol::V1,
                session_hash: None,
                login: Login::Anonymous,
                allow_messages: false,
            },
        );
        debug!("User `{}` joined the chat from {}.", id, msg.ip);
        id
    }
}
