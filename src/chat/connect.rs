use log::*;

use super::{session::Frame, ChatServer, Connection, InternalId, Login, Protocol, StateLimits, ANONYMOUS_PER_ADDRESS};
use actix::*;
use std::net::IpAddr;

#[derive(Message)]
#[rtype(result = "Option<InternalId>")]
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
    type Result = Option<InternalId>;

    fn handle(&mut self, msg: Connect, _ctx: &mut Context<Self>) -> Option<InternalId> {
        let anonymous = self
            .connections
            .values()
            .filter(|connection| connection.ip == msg.ip && connection.login == Login::Anonymous)
            .count();
        if anonymous >= ANONYMOUS_PER_ADDRESS {
            info!("Refused a connection from {}: too many without a login.", msg.ip);
            return None;
        }

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
                server_chat: false,
                minecraft: None,
                location: None,
                limits: StateLimits::default(),
            },
        );
        debug!("User `{}` joined the chat from {}.", id, msg.ip);
        Some(id)
    }
}
