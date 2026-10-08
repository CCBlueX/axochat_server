use crate::chat::{ChatServer, ClientPacket, InternalId};
use crate::error::*;
use log::*;

impl ChatServer {
    pub(super) fn send_user_count(&mut self, user_id: InternalId) {
        let Some(user) = self.logged_in(user_id) else { return };
        if !self.is_staff(user) {
            info!("`{}` tried to get the user count without permission", user_id);
            self.send_error(user_id, ClientError::NotPermitted);
            return;
        }

        self.send(
            user_id,
            ClientPacket::UserCount {
                connections: self.connections.len() as u32,
                logged_in: self.users.len() as u32,
            },
        );
    }
}
