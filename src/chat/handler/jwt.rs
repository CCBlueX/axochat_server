use super::{ChatServer, ClientPacket, Id};

use crate::error::*;
use log::*;

impl ChatServer {
    pub(super) fn handle_request_jwt(&mut self, user_id: Id) {
        let session = self
            .connections
            .get(&user_id)
            .expect("could not find connection");
        if let Some(auth) = &self.authenticator {
            if let Some(info) = &session.info {
                let token = match auth.new_token(info) {
                    Ok(token) => token,
                    Err(err) => {
                        warn!(
                            "Could not create new token for user `#{:x}`: {}",
                            user_id, err
                        );
                        session
                            .addr
                            .do_send(ClientPacket::Error(ClientError::Internal))
                            .ok();
                        return;
                    }
                };

                if let Err(err) = session.addr.do_send(ClientPacket::NewJWT(token)) {
                    warn!(
                        "Could not send mojang info to user `#{:x}`: {}",
                        user_id, err
                    );
                }
            } else {
                info!(
                    "User `#{:x}` tried to get JWT but is not logged in.",
                    user_id
                );
                session
                    .addr
                    .do_send(ClientPacket::Error(ClientError::NotLoggedIn))
                    .ok();
            }
        } else {
            info!("User `#{:x}` tried to request not supported JWT", user_id);
            session
                .addr
                .do_send(ClientPacket::Error(ClientError::NotSupported))
                .ok();
        }
    }

    pub(super) fn handle_login_jwt(&mut self, user_id: Id, jwt: &str) {
        let session = self
            .connections
            .get_mut(&user_id)
            .expect("could not find connection");
        if let Some(auth) = &self.authenticator {
            match auth.auth(jwt) {
                Ok(info) => {
                    self.users.insert(info.username.clone(), user_id);
                    session.info = Some(info);
                    if let Err(err) = session.addr.do_send(ClientPacket::LoginSuccess) {
                        info!("Could not send login success to `#{:x}`: {}", user_id, err);
                    }
                }
                Err(err) => {
                    info!("Login of user `#{:x}` using JWT failed: {}", user_id, err);
                    session
                        .addr
                        .do_send(ClientPacket::Error(ClientError::LoginFailed))
                        .ok();
                }
            };
        } else {
            info!("User `#{:x}` tried to request not supported JWT", user_id);
            session
                .addr
                .do_send(ClientPacket::Error(ClientError::NotSupported))
                .ok();
        }
    }
}
