use super::{ChatServer, ClientPacket};
use crate::auth::UserInfo;
use crate::chat::{InternalId, SessionState, send_message};

use crate::error::*;
use log::*;

impl ChatServer {
    pub(super) fn handle_message(&mut self, user_id: InternalId, content: String) {
        if self.check_ratelimit(user_id, content.clone()) {
            return;
        }

        let Some(info) = self.basic_check(user_id, &content).and_then(|session| session.user.as_ref()) else {
            return;
        };

        info!("User `{}` has written `{}`.", user_id, content);
        let client_packet = ClientPacket::Message {
            author_info: UserInfo {
                name: info.name.clone(),
                uuid: info.uuid,
            },
            content,
        };
        for session in self.connections.values() {
            send_message(
                &session.addr,
                client_packet.clone(),
                "broadcast message"
            );
        }
    }

    pub(super) fn handle_private_message(
        &mut self,
        user_id: InternalId,
        receiver: String,
        content: String,
    ) {
        if self.check_ratelimit(user_id, content.clone()) {
            return;
        }

        let Some(sender_info) = self.basic_check(user_id, &content).and_then(|session| session.user.as_ref()) else {
            return;
        };

        if let Some(receiver_user) = self.users.get(&receiver) {
            for receiver_session in receiver_user
                .connections
                .iter()
                .filter_map(|id| self.connections.get(id))
            {
                match &receiver_session.user {
                    Some(info) if info.allow_messages => {
                        let client_packet = ClientPacket::PrivateMessage {
                            author_info: UserInfo {
                                name: sender_info.name.clone(),
                                uuid: sender_info.uuid,
                            },
                            content: content.clone(),
                        };
                        info!(
                            "User `{}` has written to `{}` privately.",
                            user_id, receiver
                        );
                        if send_message(
                            &receiver_session.addr,
                            client_packet,
                            "private message"
                        ) {
                            return;
                        }
                    }
                    _ => {}
                }
            }
        } else {
            debug!(
                "User `{}` tried to write to non-existing user `{}`.",
                user_id, receiver
            );
            return;
        }

        if let Some(session) = self.connections.get(&user_id) {
            send_message(
                &session.addr,
                ClientPacket::Error {
                    message: ClientError::PrivateMessageNotAccepted,
                },
                "private message not accepted"
            );
        }
    }

    fn basic_check(&self, user_id: InternalId, content: &str) -> Option<&SessionState> {
        let session = self.connections.get(&user_id)?;

        if let Some(info) = &session.user {
            if let Err(err) = self.validator.validate(content) {
                info!("User `{}` tried to send invalid message: {}", user_id, err);
                if let Error::AxoChat { source } = err {
                    send_message(
                        &session.addr,
                        ClientPacket::Error { message: source },
                        "message validation failed"
                    );
                }

                return None;
            }
            if self.moderation.is_banned(&info.uuid) {
                info!("User `{}` tried to send message while banned", user_id);
                send_message(
                    &session.addr,
                    ClientPacket::Error {
                        message: ClientError::Banned,
                    },
                    "banned user attempted sending"
                );

                return None;
            }

            Some(session)
        } else {
            info!("`{}` is not logged in.", user_id);
            send_message(
                &session.addr,
                ClientPacket::Error {
                    message: ClientError::NotLoggedIn,
                },
                "not logged in for messaging"
            );
            None
        }
    }

    fn check_ratelimit(&mut self, user_id: InternalId, message: String) -> bool {
        let Some(session) = self.connections.get(&user_id) else {
            return true;
        };
        let Some(user) = session.user.as_ref().and_then(|user| self.users.get_mut(&user.name)) else {
            return false;
        };

        if user.rate_limiter.check_new_message(message) {
            info!(
                "User `{}` tried to send message, but was rate limited.",
                user_id
            );
            send_message(
                &session.addr,
                ClientPacket::Error {
                    message: ClientError::RateLimited,
                },
                "rate limited"
            );
            true
        } else {
            false
        }
    }
}
