use crate::error::*;
use log::*;

use super::{ChatServer, ClientPacket};
use crate::auth::UserInfo;
use crate::chat::{InternalId, SuccessReason, User, UserSession, send_message};
use crate::message::RateLimiter;
use std::collections::HashSet;

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
                        send_message(
                            &session.addr,
                            ClientPacket::Error {
                                message: ClientError::Internal,
                            },
                            "jwt creation failed"
                        );
                        return;
                    }
                };

                send_message(
                    &session.addr, 
                    ClientPacket::NewJWT { token },
                    "new jwt"
                );
            } else {
                info!("User `{}` tried to get JWT but is not logged in.", user_id);
                send_message(
                    &session.addr,
                    ClientPacket::Error {
                        message: ClientError::NotLoggedIn,
                    },
                    "jwt not logged in"
                );
            }
        } else {
            info!("User `{}` tried to request not supported JWT", user_id);
            send_message(
                &session.addr,
                ClientPacket::Error {
                    message: ClientError::NotSupported,
                },
                "jwt not supported"
            );
        }
    }

    pub(super) fn handle_login_jwt(
        &mut self,
        user_id: InternalId,
        jwt: &str,
        allow_messages: bool,
    ) {
        let session = self
            .connections
            .get_mut(&user_id)
            .expect("could not find connection");
        if let Some(auth) = &self.authenticator {
            match auth.auth(jwt) {
                Ok(info) => {
                    self.users
                        .entry(info.name.clone())
                        .or_insert(UserSession {
                            rate_limiter: RateLimiter::new(self.config.message.clone()),
                            connections: HashSet::new(),
                        })
                        .connections
                        .insert(user_id);

                    session.user = Some(User {
                        name: info.name,
                        uuid: info.uuid,
                        allow_messages,
                    });
                    
                    let addr = &session.addr;
                    send_message(
                        addr,
                        ClientPacket::Success {
                            reason: SuccessReason::Login,
                        },
                        "jwt login success"
                    );
                }
                Err(err) => {
                    info!("Login of user `{}` using JWT failed: {}", user_id, err);
                    send_message(
                        &session.addr,
                        ClientPacket::Error {
                            message: ClientError::LoginFailed,
                        },
                        "jwt login failed"
                    );
                }
            };
        } else {
            info!("User `{}` tried to request not supported JWT", user_id);
            send_message(
                &session.addr,
                ClientPacket::Error {
                    message: ClientError::NotSupported,
                },
                "jwt login not supported"
            );
        }
    }
}
