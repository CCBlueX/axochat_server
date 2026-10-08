use crate::chat::{
    now_ms, ChatServer, ClientPacket, FriendAction, FriendView, InternalId, Kind as IdentityKind, Scope, SettingsView, UserId,
    UserRef,
};
use crate::error::ClientError;
use crate::entity::relation::Kind;
use crate::store::Write;

use actix::*;

impl ChatServer {
    /// Social, party and moderation packets share a budget.
    pub(super) fn acting_user(&mut self, user_id: InternalId) -> Option<UserId> {
        let user = self.logged_in(user_id)?;
        let online = self.users.get_mut(&user)?;
        if !online.actions.allow() {
            self.send_error(user_id, ClientError::RateLimited);
            return None;
        }
        Some(user)
    }

    pub(super) fn account_user(&mut self, user_id: InternalId) -> Option<UserId> {
        let user = self.acting_user(user_id)?;
        if self.identity(user).kind != IdentityKind::Account {
            self.send_error(user_id, ClientError::AccountRequired);
            return None;
        }
        Some(user)
    }

    pub(super) fn handle_settings(
        &mut self,
        user_id: InternalId,
        allow_messages: Option<bool>,
        hide_server: Option<bool>,
        accept_friend_requests: Option<bool>,
        server_chat: Option<bool>,
    ) {
        let Some(user) = self.acting_user(user_id) else { return };
        if let Some(connection) = self.connections.get_mut(&user_id) {
            connection.allow_messages = allow_messages.unwrap_or(connection.allow_messages);
            connection.server_chat = server_chat.unwrap_or(connection.server_chat);
        }

        let Some(online) = self.users.get_mut(&user) else { return };
        let before = (online.hide_server, online.accept_friend_requests);
        online.hide_server = hide_server.unwrap_or(online.hide_server);
        online.accept_friend_requests = accept_friend_requests.unwrap_or(online.accept_friend_requests);
        let after = (online.hide_server, online.accept_friend_requests);
        if before != after {
            self.persist(vec![Write::Settings {
                user,
                hide_server: after.0,
                accept_friend_requests: after.1,
            }]);
        }
        if before.0 != after.0 {
            self.send_presence(user);
        }

        if let Some(settings) = self.settings(user_id) {
            self.send_v2(user_id, ClientPacket::Settings(settings));
        }
    }

    pub(super) fn settings(&self, user_id: InternalId) -> Option<SettingsView> {
        let connection = self.connections.get(&user_id)?;
        let online = self.users.get(&connection.user()?)?;
        Some(SettingsView {
            allow_messages: connection.allow_messages,
            hide_server: online.hide_server,
            accept_friend_requests: online.accept_friend_requests,
            server_chat: connection.server_chat,
        })
    }

    pub(super) fn handle_friend(&mut self, user_id: InternalId, action: FriendAction, query: String, ctx: &mut Context<Self>) {
        let Some(user) = self.account_user(user_id) else { return };
        self.resolve_user(ctx, user, query, Scope::Accounts, move |actor, _ctx, resolved| {
            // a name nobody has answers like a real account would
            let Some(target) = resolved else {
                match action {
                    FriendAction::Request => {}
                    FriendAction::Accept | FriendAction::Decline => actor.send_error(user_id, ClientError::NoInvite),
                    FriendAction::Remove => actor.send_error(user_id, ClientError::NotFriends),
                }
                return;
            };
            let (target_id, now) = (target.identity.id, now_ms());
            let result = match action {
                FriendAction::Request => actor
                    .social
                    .request(user, target_id, target.accept_friend_requests, now)
                    .map(|(_, writes)| writes),
                FriendAction::Accept => actor.social.accept(user, target_id, now),
                FriendAction::Decline => actor.social.decline(user, target_id, now),
                FriendAction::Remove => actor.social.remove(user, target_id, now),
            };
            match result {
                Ok(writes) => {
                    actor.persist(writes);
                    actor.send_friends(user);
                    actor.send_friends(target_id);
                }
                Err(error) => actor.send_error(user_id, error),
            }
        });
    }

    pub(super) fn handle_block(&mut self, user_id: InternalId, query: String, blocked: bool, ctx: &mut Context<Self>) {
        let Some(user) = self.acting_user(user_id) else { return };
        self.resolve_user(ctx, user, query, Scope::Anyone, move |actor, _ctx, resolved| {
            let Some(target) = resolved else { return };
            let target_id = target.identity.id;
            // the blocked user only hears of it through a friendship or request that disappears
            let target_affected = actor.social.get(user, target_id).is_some_and(|kind| kind != Kind::Block)
                || actor.social.get(target_id, user).is_some_and(|kind| kind != Kind::Block);
            match actor.social.block(user, target_id, blocked, now_ms()) {
                Ok(writes) => {
                    actor.persist(writes);
                    actor.send_friends(user);
                    if blocked && target_affected {
                        actor.send_friends(target_id);
                    }
                    actor.send_blocks(user);
                }
                Err(error) => actor.send_error(user_id, error),
            }
        });
    }

    pub(super) fn send_friends(&self, user: UserId) {
        if !self.users.contains_key(&user) {
            return;
        }
        let refs = |users: Vec<UserId>| -> Vec<UserRef> {
            users
                .into_iter()
                .filter_map(|user| self.known_ref(user))
                .collect()
        };
        let friends = self
            .social
            .friends(user)
            .filter_map(|(friend, since)| {
                Some(FriendView {
                    user: self.known_ref(friend)?,
                    since,
                    online: self.users.contains_key(&friend),
                    server: self.visible_server(friend),
                })
            })
            .collect();
        let packet = ClientPacket::Friends {
            friends,
            incoming: refs(self.social.incoming_requests(user).collect()),
            outgoing: refs(self.social.outgoing_requests(user).collect()),
        };
        for (id, _) in self.online_connections(user) {
            self.send_v2(id, packet.clone());
        }
    }

    pub(super) fn send_blocks(&self, user: UserId) {
        let users = self
            .social
            .blocked(user)
            .filter_map(|blocked| self.known_ref(blocked))
            .collect();
        let packet = ClientPacket::Blocks { users };
        for (id, _) in self.online_connections(user) {
            self.send_v2(id, packet.clone());
        }
    }

    pub(super) fn visible_server(&self, user: UserId) -> Option<String> {
        if self.users.get(&user)?.hide_server {
            return None;
        }
        self.game_location(user)?.address.clone()
    }

    pub(in crate::chat) fn send_presence(&self, user: UserId) {
        let packet = ClientPacket::Presence {
            user,
            online: self.users.contains_key(&user),
            server: self.visible_server(user),
        };
        for (friend, _) in self.social.friends(user) {
            for (id, _) in self.online_connections(friend) {
                self.send_v2(id, packet.clone());
            }
        }
    }
}
