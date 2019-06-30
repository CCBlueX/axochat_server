use super::{ChatServer, ClientPacket, Id};
use crate::chat::InternalId;

use crate::error::*;
use log::*;

impl ChatServer {
    pub(super) fn ban_user(&mut self, user_id: InternalId, to_ban: Id) {
        self.handle_user(user_id, to_ban, true);
    }

    pub(super) fn unban_user(&mut self, user_id: InternalId, to_unban: Id) {
        self.handle_user(user_id, to_unban, false);
    }

    fn handle_user(&mut self, user_id: InternalId, receiver: Id, ban: bool) {
        let session = self
            .connections
            .get(&user_id)
            .expect("could not find connection");
        if let Some(info) = &session.user {
            if !self.moderation.is_moderator(&info.uuid.into()) {
                info!("`{}` tried to (un-)ban user without permission", user_id);
                session
                    .addr
                    .do_send(ClientPacket::Error {
                        message: ClientError::NotPermitted,
                    })
                    .ok();
                return;
            }

            let res = if ban {
                self.moderation.ban(&receiver)
            } else {
                self.moderation.unban(&receiver)
            };
            match res {
                Ok(()) => {
                    if ban {
                        info!("User `{}` banned.", receiver);
                    } else {
                        info!("User `{}` unbanned.", receiver);
                    }
                    session.addr.do_send(ClientPacket::Success).ok();
                }
                Err(Error::AxoChat(err)) => {
                    info!("Could not (un-)ban user `{}`: {}", receiver, err);
                    session
                        .addr
                        .do_send(ClientPacket::Error { message: err })
                        .ok();
                }
                Err(err) => {
                    info!("Could not (un-)ban user `{}`: {}", receiver, err);
                    session
                        .addr
                        .do_send(ClientPacket::Error {
                            message: ClientError::Internal,
                        })
                        .ok();
                }
            }
        } else {
            info!("`{}` is not logged in.", user_id);
            session
                .addr
                .do_send(ClientPacket::Error {
                    message: ClientError::NotLoggedIn,
                })
                .ok();
            return;
        }
    }
}
