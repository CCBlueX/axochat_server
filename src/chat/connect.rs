use log::*;

use super::{ChatServer, ClientPacket, SessionState};
use actix::*;

use crate::message::RateLimiter;
use rand::{Rng, RngCore};

#[derive(Message)]
#[rtype(usize)]
pub(in crate::chat) struct Connect {
    addr: Recipient<ClientPacket>,
}

impl Connect {
    pub fn new(addr: Recipient<ClientPacket>) -> Connect {
        Connect { addr }
    }
}

impl Handler<Connect> for ChatServer {
    type Result = usize;

    fn handle(&mut self, msg: Connect, ctx: &mut Context<Self>) -> usize {
        use hashbrown::hash_map::Entry;

        let session_hash = {
            let mut bytes = [0; 20];
            self.rng.fill_bytes(&mut bytes);
            // we'll just ignore one bit so we that don't have to deal with a '-' sign
            bytes[0] &= 0b0111_1111;

            crate::auth::encode_sha1_bytes(&bytes)
        };

        msg.addr
            .send(ClientPacket::ServerInfo {
                session_hash: session_hash.clone(),
            })
            .into_actor(self)
            .then(|res, _actor, ctx| {
                match res {
                    Ok(()) => {}
                    Err(err) => {
                        warn!("Could not send session hash: {}", err);
                        ctx.stop();
                    }
                }
                fut::ok(())
            })
            .wait(ctx);

        loop {
            let id = self.rng.gen();
            match self.connections.entry(id) {
                Entry::Occupied(_) => {}
                Entry::Vacant(v) => {
                    v.insert(SessionState {
                        addr: msg.addr.clone(),
                        session_hash,
                        username: String::new(),
                        anonymous: true,
                        rate_limiter: RateLimiter::new(self.config.message.clone()),
                    });
                    debug!("User with id {:x} joined the chat.", id);
                    return id;
                }
            }
        }
    }
}
