use super::login::{identify, Verified};
use crate::api::ApiError;
use crate::chat::{ChatServer, InternalId};
use crate::error::ClientError;
use crate::store::IdentityKey;
use log::*;

use actix::*;

impl ChatServer {
    pub(super) fn login_account(&mut self, user_id: InternalId, token: String, allow_messages: bool, ctx: &mut Context<Self>) {
        let Some(visit) = self.begin_login(user_id) else { return };
        let (api, store, logins) = (self.api.clone(), self.store.clone(), self.logins.clone());

        ctx.spawn(
            async move {
                let _permit = logins.acquire().await;
                let account = api.account(&token).await.map_err(|err| match err {
                    ApiError::Unauthorized => ClientError::LoginFailed,
                    ApiError::Unavailable(reason) => {
                        warn!("Could not check account of `{}`: {}", user_id, reason);
                        ClientError::Internal
                    }
                })?;

                let request = visit.identify(IdentityKey::Account(account.user_id), account.nickname, account.minecraft_uuid);
                let model = identify(store, request).await?;
                Ok(Verified { model, roles: account.roles })
            }
            .into_actor(self)
            .map(move |result, actor, _ctx| actor.finish_login(user_id, result, allow_messages)),
        );
    }
}
