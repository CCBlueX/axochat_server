use super::{AtUser, ChatServer, ClientPacket, Id};
use crate::chat::SessionState;

use crate::error::*;
use log::*;

impl ChatServer {
    pub(super) fn ban_user(&mut self, user_id: Id, to_ban: AtUser) {
        self.handle_user(user_id, to_ban, true);
    }

    pub(super) fn unban_user(&mut self, user_id: Id, to_unban: AtUser) {
        self.handle_user(user_id, to_unban, false);
    }

    fn handle_user(&mut self, user_id: Id, receiver: AtUser, ban: bool) {
        let session = self
            .connections
            .get(&user_id)
            .expect("could not find connection");
        if let Some(info) = &session.info {
            if !self.moderation.is_moderator(&info.username) {
                info!("`{}` tried to (un-)ban user without permission", user_id);
                session
                    .addr
                    .do_send(ClientPacket::Error(ClientError::NotPermitted))
                    .ok();
                return;
            }

            let username = match receiver {
                AtUser::Id(id) => match self.connections.get(&id) {
                    Some(SessionState {
                        info: Some(info), ..
                    }) => info.username.clone(),
                    _ => {
                        info!("Could not find user `{}`", id);
                        session
                            .addr
                            .do_send(ClientPacket::Error(ClientError::InvalidUser))
                            .ok();
                        return;
                    }
                },
                AtUser::Name(name) => {
                    if is_name_valid(&name) {
                        name
                    } else {
                        info!("Invalid username `{}`", name);
                        session
                            .addr
                            .do_send(ClientPacket::Error(ClientError::InvalidUser))
                            .ok();
                        return;
                    }
                }
            };

            let res = if ban {
                self.moderation.ban(&username)
            } else {
                self.moderation.unban(&username)
            };
            match res {
                Ok(()) => {
                    if ban {
                        info!("User `{}` banned.", username);
                    } else {
                        info!("User `{}` unbanned.", username);
                    }
                    session.addr.do_send(ClientPacket::Success).ok();
                }
                Err(Error::AxoChat(err)) => {
                    info!("Could not (un-)ban user `{}`: {}", username, err);
                    session.addr.do_send(ClientPacket::Error(err)).ok();
                }
                Err(err) => {
                    info!("Could not (un-)ban user `{}`: {}", username, err);
                    session
                        .addr
                        .do_send(ClientPacket::Error(ClientError::Internal))
                        .ok();
                }
            }
        } else {
            info!("`{}` is not logged in.", user_id);
            session
                .addr
                .do_send(ClientPacket::Error(ClientError::NotLoggedIn))
                .ok();
            return;
        }
    }
}

fn is_name_valid(username: &str) -> bool {
    if username.is_empty() || username.len() > 16 {
        return false;
    }

    for ch in username.chars() {
        if !ch.is_ascii_alphanumeric() && ch != '_' {
            return false;
        }
    }

    true
}
