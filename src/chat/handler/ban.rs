use super::{ChatServer, ClientPacket};
use crate::chat::{InternalId, SuccessReason};

use crate::error::*;
use log::*;
use uuid::Uuid;

impl ChatServer {
    pub(super) fn ban_user(&mut self, user_id: InternalId, to_ban: &Uuid) {
        self.handle_user(user_id, to_ban, true);
    }

    pub(super) fn unban_user(&mut self, user_id: InternalId, to_unban: &Uuid) {
        self.handle_user(user_id, to_unban, false);
    }

    fn handle_user(&mut self, user_id: InternalId, receiver: &Uuid, ban: bool) {
        let session = self
            .connections
            .get(&user_id)
            .expect("could not find connection");
        if let Some(info) = &session.user {
            if !self.moderation.is_moderator(&info.uuid) {
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
                self.moderation.ban(receiver)
            } else {
                self.moderation.unban(receiver)
            };
            match res {
                Ok(()) => {
                    let reason = if ban {
                        info!("User `{}` banned.", receiver);
                        SuccessReason::Ban
                    } else {
                        info!("User `{}` unbanned.", receiver);
                        SuccessReason::Unban
                    };
                    let _ = session.addr.do_send(ClientPacket::Success { reason });
                }
                Err(Error::AxoChat { source }) => {
                    info!("Could not (un-)ban user `{}`: {}", receiver, source);
                    session
                        .addr
                        .do_send(ClientPacket::Error { message: source })
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
