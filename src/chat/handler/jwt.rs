use super::{ChatServer, ClientPacket};
use crate::auth::UserInfo;
use crate::chat::{InternalId, User};

use crate::error::*;
use log::*;

impl ChatServer {
    pub(super) fn handle_request_jwt(&mut self, user_id: InternalId) {
        let session = self
            .connections
            .get(&user_id)
            .expect("could not find connection");
        if let Some(auth) = &self.authenticator {
            if let Some(user) = &session.user {
                let token = match auth.new_token(UserInfo {
                    name: user.name.clone(),
                    uuid: user.uuid,
                }) {
                    Ok(token) => token,
                    Err(err) => {
                        warn!("Could not create new token for user `{}`: {}", user_id, err);
                        session
                            .addr
                            .do_send(ClientPacket::Error {
                                message: ClientError::Internal,
                            })
                            .ok();
                        return;
                    }
                };

                if let Err(err) = session.addr.do_send(ClientPacket::NewJWT { token }) {
                    warn!("Could not send mojang info to user `{}`: {}", user_id, err);
                }
            } else {
                info!("User `{}` tried to get JWT but is not logged in.", user_id);
                session
                    .addr
                    .do_send(ClientPacket::Error {
                        message: ClientError::NotLoggedIn,
                    })
                    .ok();
            }
        } else {
            info!("User `{}` tried to request not supported JWT", user_id);
            session
                .addr
                .do_send(ClientPacket::Error {
                    message: ClientError::NotSupported,
                })
                .ok();
        }
    }

    pub(super) fn handle_login_jwt(
        &mut self,
        user_id: InternalId,
        jwt: &str,
        anonymous: bool,
        allow_messages: bool,
    ) {
        let session = self
            .connections
            .get_mut(&user_id)
            .expect("could not find connection");
        if let Some(auth) = &self.authenticator {
            match auth.auth(jwt) {
                Ok(info) => {
                    self.ids.insert(info.uuid.into(), user_id);
                    session.user = Some(User {
                        name: info.name,
                        uuid: info.uuid,
                        anonymous,
                        allow_messages,
                    });
                    if let Err(err) = session.addr.do_send(ClientPacket::Success) {
                        info!("Could not send login success to `{}`: {}", user_id, err);
                    }
                }
                Err(err) => {
                    info!("Login of user `{}` using JWT failed: {}", user_id, err);
                    session
                        .addr
                        .do_send(ClientPacket::Error {
                            message: ClientError::LoginFailed,
                        })
                        .ok();
                }
            };
        } else {
            info!("User `{}` tried to request not supported JWT", user_id);
            session
                .addr
                .do_send(ClientPacket::Error {
                    message: ClientError::NotSupported,
                })
                .ok();
        }
    }
}
