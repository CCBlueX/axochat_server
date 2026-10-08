use crate::chat::{send_message, ChatServer, ClientPacket, InternalId, Login, UserId};
use crate::error::*;
use log::*;

impl ChatServer {
    pub(super) fn handle_message(&mut self, user_id: InternalId, content: String) {
        let Some(user) = self.logged_in(user_id) else { return };
        if !self.check_message(user_id, user, &content) {
            return;
        }

        info!("User `{}` has written `{}`.", user_id, content);
        let packet = ClientPacket::Message {
            author_info: self.identity(user).info(),
            content,
        };
        for connection in self.connections.values() {
            if matches!(connection.login, Login::User(_)) {
                send_message(connection, packet.clone());
            }
        }
    }

    pub(super) fn handle_private_message(&mut self, user_id: InternalId, receiver: String, content: String) {
        let Some(user) = self.logged_in(user_id) else { return };
        if !self.check_message(user_id, user, &content) {
            return;
        }

        let Some(receiver) = self.find_online(&receiver) else {
            debug!("User `{}` tried to write to non-existing user `{}`.", user_id, receiver);
            return;
        };

        let packet = ClientPacket::PrivateMessage {
            author_info: self.identity(user).info(),
            content,
        };
        let mut delivered = false;
        for (_, connection) in self.online_connections(receiver) {
            if connection.allow_messages {
                delivered |= send_message(connection, packet.clone());
            }
        }

        if delivered {
            info!("User `{}` has written to `{}` privately.", user_id, receiver);
        } else {
            self.send_error(user_id, ClientError::PrivateMessageNotAccepted);
        }
    }

    fn check_message(&mut self, user_id: InternalId, user: UserId, content: &str) -> bool {
        let Some(online) = self.users.get_mut(&user) else { return false };
        if online.rate_limiter.check_new_message(content.to_owned()) {
            info!("User `{}` tried to send message, but was rate limited.", user_id);
            self.send_error(user_id, ClientError::RateLimited);
            return false;
        }

        if let Err(err) = self.validator.validate(content) {
            info!("User `{}` tried to send invalid message: {}", user_id, err);
            if let Error::AxoChat { source } = err {
                self.send_error(user_id, source);
            }
            return false;
        }

        if self.moderation.is_banned(&self.identity(user).uuid) {
            info!("User `{}` tried to send message while banned", user_id);
            self.send_error(user_id, ClientError::Banned);
            return false;
        }

        true
    }
}
