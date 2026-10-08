use crate::chat::{ChatServer, ClientPacket, InternalId, Login, Malformed, Protocol, PROTOCOL};
use crate::error::ClientError;
use log::*;

use actix::*;

impl ChatServer {
    pub(super) fn handle_hello(&mut self, user_id: InternalId, protocol: u32) {
        let Some(connection) = self.connections.get_mut(&user_id) else { return };
        if connection.login != Login::Anonymous {
            return;
        }

        if protocol >= PROTOCOL {
            connection.protocol = Protocol::V2;
        }
        let protocol = match connection.protocol {
            Protocol::V1 => 1,
            Protocol::V2 => PROTOCOL,
        };
        self.send(user_id, ClientPacket::Hello { protocol });
    }
}

impl Handler<Malformed> for ChatServer {
    type Result = ();

    fn handle(&mut self, Malformed { user_id, error }: Malformed, _ctx: &mut Context<Self>) {
        debug!("Could not decode packet of `{}`: {}", user_id, error);
        let Some(connection) = self.connections.get(&user_id) else { return };
        if connection.protocol >= Protocol::V2 {
            let detail: String = error.chars().take(200).collect();
            self.send(user_id, ClientPacket::error_with(ClientError::InvalidPacket, detail));
        }
    }
}
