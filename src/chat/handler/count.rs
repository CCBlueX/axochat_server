use crate::chat::{ChatServer, ClientPacket, InternalId};
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
                let _ = session.addr.do_send(ClientPacket::Error {
                    message: ClientError::NotPermitted,
                });
                return;
            }

            if let Err(err) = session.addr.do_send(ClientPacket::UserCount {
                connections: self.connections.len() as u32,
                logged_in: self.users.values().map(|user| user.connections.len() as u32).sum(),
            }) {
                warn!("Could not send user count to user `{}`: {}", user_id, err);
            }
        } else {
            info!("`{}` is not logged in.", user_id);
            let _ = session.addr.do_send(ClientPacket::Error {
                message: ClientError::NotLoggedIn,
            });
        }
    }
}
