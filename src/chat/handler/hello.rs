use crate::chat::{send_message, ChatServer, ClientPacket, InternalId, Malformed, Protocol, PROTOCOL};
use crate::error::ClientError;
use log::*;

use actix::*;

impl ChatServer {
    pub(super) fn handle_hello(&mut self, user_id: InternalId, protocol: u32) {
        let Some(session) = self.connections.get_mut(&user_id) else { return };
        if session.is_logged_in() || session.login_pending {
            return;
        }

        if protocol >= PROTOCOL {
            session.protocol = Protocol::V2;
        }
        let protocol = match session.protocol {
            Protocol::V1 => 1,
            Protocol::V2 => PROTOCOL,
        };
        send_message(session, ClientPacket::Hello { protocol }, "hello");
    }
}

impl Handler<Malformed> for ChatServer {
    type Result = ();

    fn handle(&mut self, Malformed { user_id, error }: Malformed, _ctx: &mut Context<Self>) {
        debug!("Could not decode packet of `{}`: {}", user_id, error);
        let Some(session) = self.connections.get(&user_id) else { return };
        if session.protocol >= Protocol::V2 {
            let detail: String = error.chars().take(200).collect();
            send_message(session, ClientPacket::error_with(ClientError::InvalidPacket, detail), "invalid packet");
        }
    }
}
