use super::{ChatServer, ClientPacket};
use crate::chat::{InternalId, SuccessReason, send_message};

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
                send_message(
                    &session.addr,
                    ClientPacket::Error {
                        message: ClientError::NotPermitted,
                    },
                    "permission denied"
                );
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
                    send_message(
                        &session.addr, 
                        ClientPacket::Success { reason },
                        "ban/unban success"
                    );
                }
                Err(Error::AxoChat { source }) => {
                    info!("Could not (un-)ban user `{}`: {}", receiver, source);
                    send_message(
                        &session.addr,
                        ClientPacket::Error { message: source },
                        "ban/unban client error"
                    );
                }
                Err(err) => {
                    info!("Could not (un-)ban user `{}`: {}", receiver, err);
                    send_message(
                        &session.addr,
                        ClientPacket::Error {
                            message: ClientError::Internal,
                        },
                        "ban/unban internal error"
                    );
                }
            }
        } else {
            info!("`{}` is not logged in.", user_id);
            send_message(
                &session.addr,
                ClientPacket::Error {
                    message: ClientError::NotLoggedIn,
                },
                "not logged in"
            );
            return;
        }
    }
}
