use crate::chat::{ChatServer, ClientPacket, InternalId, SuccessReason};

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
        let Some(user) = self.logged_in(user_id) else { return };
        if !self.is_staff(user) {
            info!("`{}` tried to (un-)ban user without permission", user_id);
            self.send_error(user_id, ClientError::NotPermitted);
            return;
        }

        let protected = self
            .users
            .keys()
            .any(|online| self.identity(*online).uuid == *receiver && self.is_staff(*online));
        if ban && protected {
            self.send_error(user_id, ClientError::NotPermitted);
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
                self.send(user_id, ClientPacket::Success { reason });
            }
            Err(Error::AxoChat { source }) => {
                info!("Could not (un-)ban user `{}`: {}", receiver, source);
                self.send_error(user_id, source);
            }
            Err(err) => {
                info!("Could not (un-)ban user `{}`: {}", receiver, err);
                self.send_error(user_id, ClientError::Internal);
            }
        }
    }
}
