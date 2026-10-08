use super::login::{identify, Verified};
use crate::chat::{ChatServer, ClientPacket, InternalId};
use crate::error::*;
use crate::store::IdentityKey;
use log::*;

use actix::*;

impl ChatServer {
    pub(super) fn handle_request_jwt(&mut self, user_id: InternalId) {
        let Some(auth) = &self.authenticator else {
            self.send_error(user_id, ClientError::NotSupported);
            return;
        };
        let Some(user) = self.logged_in(user_id) else { return };

        match auth.new_token(self.identity(user).info()) {
            Ok(token) => {
                self.send(user_id, ClientPacket::NewJWT { token });
            }
            Err(err) => {
                warn!("Could not create new token for user `{}`: {}", user_id, err);
                self.send_error(user_id, ClientError::Internal);
            }
        }
    }

    pub(super) fn handle_login_jwt(&mut self, user_id: InternalId, jwt: &str, allow_messages: bool, ctx: &mut Context<Self>) {
        let Some(auth) = &self.authenticator else {
            self.send_error(user_id, ClientError::NotSupported);
            return;
        };
        let info = match auth.auth(jwt) {
            Ok(info) => info,
            Err(err) => {
                info!("Login of user `{}` using JWT failed: {}", user_id, err);
                self.send_error(user_id, ClientError::LoginFailed);
                return;
            }
        };
        if !self.begin_login(user_id) {
            return;
        }

        let store = self.store.clone();
        let Some(request) = self.identify_request(user_id, IdentityKey::Minecraft(info.uuid), info.name, None) else {
            return;
        };
        ctx.spawn(
            async move { identify(store, request).await.map(|model| Verified { model }) }
                .into_actor(self)
                .map(move |result, actor, _ctx| actor.finish_login(user_id, result, allow_messages)),
        );
    }
}
