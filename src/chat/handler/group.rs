use super::message::send_frame;
use crate::chat::{
    new_id, now_ms, Channel, ChatServer, ClientPacket, GroupAction, GroupId, GroupMemberView, GroupView, InternalId,
    Protocol, Scope, UserId,
};
use crate::entity::chat_group_member::Role;
use crate::error::ClientError;
use crate::store::Write;

use actix::*;

impl ChatServer {
    pub(super) fn handle_group(&mut self, user_id: InternalId, action: GroupAction, ctx: &mut Context<Self>) {
        let Some(user) = self.account_user(user_id) else { return };
        let now = now_ms();
        match action {
            GroupAction::Create { name } => {
                let id = new_id(&mut self.rng);
                let result = self.groups.create(user, id, name, now);
                self.group_changed(user_id, id, Vec::new(), result);
            }
            GroupAction::Rename { group, name } => {
                let result = self.groups.rename(user, group, name);
                self.group_changed(user_id, group, Vec::new(), result);
            }
            GroupAction::Accept { group } => {
                let result = self.groups.accept(user, group, now);
                self.group_changed(user_id, group, Vec::new(), result);
            }
            GroupAction::Decline { group } | GroupAction::Leave { group } => {
                let before = self.groups.affected(group);
                let result = self.groups.leave(user, group);
                self.group_changed(user_id, group, before, result);
            }
            GroupAction::Delete { group } => {
                let before = self.groups.affected(group);
                let result = self.groups.delete(user, group);
                self.group_changed(user_id, group, before, result);
            }
            GroupAction::Invite { group, user: query } => self.resolve_user(ctx, query, Scope::Accounts, move |actor, _ctx, resolved| {
                // only friends can be invited, so nobody else is worth telling apart
                let Some(target) = resolved else {
                    actor.send_error(user_id, ClientError::NotFriends);
                    return;
                };
                let target = target.identity.id;
                let friends = actor.social.are_friends(user, target);
                let blocked = actor.social.has_blocked(target, user);
                let result = actor.groups.invite(user, group, target, friends, blocked, now);
                actor.group_changed(user_id, group, Vec::new(), result);
            }),
            GroupAction::Kick { group, user: query } => self.resolve_user(ctx, query.clone(), Scope::Accounts, move |actor, _ctx, resolved| {
                let Some(target) = resolved else {
                    actor.send(user_id, ClientPacket::error_with(ClientError::UnknownUser, query));
                    return;
                };
                let before = actor.groups.affected(group);
                let result = actor.groups.kick(user, group, target.identity.id);
                actor.group_changed(user_id, group, before, result);
            }),
            GroupAction::Promote { group, user: query, admin } => {
                self.resolve_user(ctx, query.clone(), Scope::Accounts, move |actor, _ctx, resolved| {
                    let Some(target) = resolved else {
                        actor.send(user_id, ClientPacket::error_with(ClientError::UnknownUser, query));
                        return;
                    };
                    let result = actor.groups.promote(user, group, target.identity.id, admin, now);
                    actor.group_changed(user_id, group, Vec::new(), result);
                })
            }
        }
    }

    fn group_changed(&mut self, user_id: InternalId, group: GroupId, mut affected: Vec<UserId>, result: Result<Vec<Write>, ClientError>) {
        match result {
            Ok(writes) => {
                self.persist(writes);
                affected.extend(self.groups.affected(group));
                affected.sort_unstable();
                affected.dedup();
                for user in affected {
                    self.send_groups(user);
                }
            }
            Err(error) => self.send_error(user_id, error),
        }
    }

    pub(in crate::chat) fn send_groups(&self, user: UserId) {
        if !self.users.contains_key(&user) {
            return;
        }
        let groups = self
            .groups
            .of(user)
            .filter_map(|(id, group)| {
                let mut members: Vec<GroupMemberView> = group
                    .members
                    .iter()
                    .filter_map(|(member, (role, _))| {
                        Some(GroupMemberView {
                            user: self.known_ref(*member)?,
                            role: *role,
                            online: self.users.contains_key(member),
                        })
                    })
                    .collect();
                members.sort_by_key(|member| member.user.name.to_lowercase());
                Some(GroupView {
                    id,
                    name: group.name.clone(),
                    role: group.role(user)?,
                    members,
                })
            })
            .collect();
        let packet = ClientPacket::Groups { groups };
        for (id, _) in self.online_connections(user) {
            self.send_v2(id, packet.clone());
        }
    }

    pub(super) fn send_group_message(&mut self, user_id: InternalId, user: UserId, group: GroupId, content: String) {
        let members: Vec<UserId> = match self.groups.get(group) {
            Some(entry) if !matches!(entry.role(user), None | Some(Role::Invited)) => entry.joined().collect(),
            _ => {
                self.send(user_id, ClientPacket::error_with(ClientError::UnknownGroup, group.to_string()));
                return;
            }
        };

        let channel = Channel::Group(group);
        let message = self.record(channel.clone(), user, &content, Some(members.clone()));
        let frame = self.chat_frame(message, channel);
        for member in members {
            if self.social.has_blocked(member, user) {
                continue;
            }
            for (_, connection) in self.online_connections(member) {
                if connection.protocol >= Protocol::V2 {
                    send_frame(connection, &frame);
                }
            }
        }
    }
}
