use super::{AtUser, ChatServer, ClientPacket, Id};

use crate::error::*;
use log::*;

impl ChatServer {
    pub(super) fn handle_message(&mut self, user_id: Id, content: String) {
        info!("{:x} has written `{}`.", user_id, content);
        let session = self
            .connections
            .get_mut(&user_id)
            .expect("could not find connection");
        if !session.is_logged_in() {
            info!("{:x} is not logged in.", user_id);
            session
                .addr
                .do_send(ClientPacket::Error(ClientError::NotLoggedIn))
                .ok();
            return;
        }
        if let Err(err) = self.validator.validate(&content) {
            info!("`#{:x}` tried to send invalid message: {}", user_id, err);
            if let Error::AxoChat(err) = err {
                session
                    .addr
                    .do_send(ClientPacket::Error(err))
                    .ok();
            }

            return;
        }

        if session.rate_limiter.check_new_message() {
            info!("{:x} tried to send message, but was rate limited.", user_id);
            session
                .addr
                .do_send(ClientPacket::Error(ClientError::RateLimited))
                .ok();
            return;
        }

        let client_packet = ClientPacket::Message {
            author_id: user_id,
            author_name: session.username_opt(),
            content,
        };
        for session in self.connections.values() {
            if !session.is_logged_in() {
                if let Err(err) = session.addr.do_send(client_packet.clone()) {
                    warn!("Could not send message to client: {}", err);
                }
            }
        }
    }

    pub(super) fn handle_private_message(
        &mut self,
        user_id: Id,
        receiver: AtUser,
        content: String,
    ) {
        info!("{:x} has written to `{}`.", user_id, receiver);
        debug!("{:x} has written `{}` to `{}`.", user_id, content, receiver);
        let sender_session = self
            .connections
            .get(&user_id)
            .expect("could not find connection");
        if !sender_session.is_logged_in() {
            info!("{:x} is not logged in.", user_id);
            sender_session
                .addr
                .do_send(ClientPacket::Error(ClientError::NotLoggedIn))
                .ok();
            return;
        }
        if let Err(err) = self.validator.validate(&content) {
            info!("`#{:x}` tried to send invalid message: {}", user_id, err);
            if let Error::AxoChat(err) = err {
                sender_session
                    .addr
                    .do_send(ClientPacket::Error(err))
                    .ok();
            }

            return;
        }
        let receiver_session = match self.get_connection(&receiver) {
            Some(ses) => ses,
            None => {
                debug!(
                    "{:x} tried to write to non-existing user `{}`.",
                    user_id, receiver
                );
                return;
            }
        };

        match &receiver_session.info {
            Some(info) if info.allow_messages => {
                let client_packet = ClientPacket::PrivateMessage {
                    author_id: user_id,
                    author_name: sender_session.username_opt(),
                    content,
                };
                if let Err(err) = receiver_session.addr.do_send(client_packet) {
                    warn!("Could not send private message to client: {}", err);
                } else {
                    return;
                }
            }
            _ => {}
        }

        sender_session
            .addr
            .do_send(ClientPacket::Error(ClientError::PrivateMessageNotAccepted))
            .ok();
    }
}
