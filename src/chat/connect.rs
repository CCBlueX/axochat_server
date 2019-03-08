use log::*;

use super::{ChatServer, ClientPacket, Id, SessionState};
use actix::*;

use crate::message::RateLimiter;
use rand::Rng;

#[derive(Message)]
#[rtype(Id)]
pub(in crate::chat) struct Connect {
    addr: Recipient<ClientPacket>,
}

impl Connect {
    pub fn new(addr: Recipient<ClientPacket>) -> Connect {
        Connect { addr }
    }
}

impl Handler<Connect> for ChatServer {
    type Result = Id;

    fn handle(&mut self, msg: Connect, _ctx: &mut Context<Self>) -> Id {
        use hashbrown::hash_map::Entry;

        loop {
            let id = Id(self.rng.gen());
            match self.connections.entry(id) {
                Entry::Occupied(_) => {}
                Entry::Vacant(v) => {
                    v.insert(SessionState {
                        addr: msg.addr.clone(),
                        session_hash: None,
                        info: None,
                        rate_limiter: RateLimiter::new(self.config.message.clone()),
                    });
                    debug!("User `{}` joined the chat.", id);
                    return id;
                }
            }
        }
    }
}
