use crate::chat::{ChatServer, ClientPacket, InternalId, send_message};
use crate::error::*;
use log::*;

impl ChatServer {
    pub(super) fn send_user_count(&mut self, user_id: InternalId) {
        let session = self
            .connections
            .get(&user_id)
            .expect("could not find connection");

        if let Some(info) = &session.user {
            if !self.moderation.is_moderator(&info.uuid) {
                info!(
                    "`{}` tried to get the user count without permission",
                    user_id
                );
                send_message(
                    &session.addr,
                    ClientPacket::Error {
                        message: ClientError::NotPermitted,
                    },
                    "permission denied for user count"
                );
                return;
            }

            send_message(
                &session.addr,
                ClientPacket::UserCount {
                    connections: self.connections.len() as u32,
                    logged_in: self.users.len() as u32,
                },
                "user count"
            );
        } else {
            info!("`{}` is not logged in.", user_id);
            send_message(
                &session.addr,
                ClientPacket::Error {
                    message: ClientError::NotLoggedIn,
                },
                "not logged in for user count"
            );
        }
    }
}
