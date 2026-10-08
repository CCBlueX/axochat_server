use super::UserId;
use crate::entity::chat_group_member::Role;
use crate::entity::{chat_group, chat_group_member};
use crate::error::ClientError;
use crate::store::Write;

use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub type GroupId = Uuid;

/// Members and pending invites together.
pub const MAX_MEMBERS: usize = 32;
const MAX_GROUPS: usize = 16;
const MAX_NAME: usize = 24;

pub struct Group {
    pub name: String,
    pub owner: UserId,
    pub members: HashMap<UserId, (Role, i64)>,
}

impl Group {
    pub fn role(&self, user: UserId) -> Option<Role> {
        self.members.get(&user).map(|(role, _)| *role)
    }

    /// Members who joined, not those only invited.
    pub fn joined(&self) -> impl Iterator<Item = UserId> + '_ {
        self.members
            .iter()
            .filter(|(_, (role, _))| *role != Role::Invited)
            .map(|(user, _)| *user)
    }
}

#[derive(Default)]
pub struct Groups {
    groups: HashMap<GroupId, Group>,
    memberships: HashMap<UserId, HashSet<GroupId>>,
}

pub fn valid_name(name: &str) -> bool {
    let length = name.chars().count();
    (1..=MAX_NAME).contains(&length)
        && name.trim() == name
        && name.chars().all(|ch| ch == ' ' || ch.is_ascii_graphic() || ch.is_alphanumeric())
}

fn manages(role: Option<Role>) -> bool {
    matches!(role, Some(Role::Owner | Role::Admin))
}

impl Groups {
    pub fn new(groups: Vec<chat_group::Model>, members: Vec<chat_group_member::Model>) -> Groups {
        let mut loaded = Groups::default();
        for group in groups {
            loaded.groups.insert(
                group.id,
                Group {
                    name: group.name,
                    owner: group.owner_id,
                    members: HashMap::new(),
                },
            );
        }
        for member in members {
            if let Some(group) = loaded.groups.get_mut(&member.group_id) {
                group.members.insert(member.user_id, (member.role, member.joined_at));
                loaded.memberships.entry(member.user_id).or_default().insert(member.group_id);
            }
        }
        loaded
    }

    pub fn get(&self, id: GroupId) -> Option<&Group> {
        self.groups.get(&id)
    }

    pub fn of(&self, user: UserId) -> impl Iterator<Item = (GroupId, &Group)> + '_ {
        self.memberships
            .get(&user)
            .into_iter()
            .flatten()
            .filter_map(|id| self.groups.get(id).map(|group| (*id, group)))
    }

    fn set_member(&mut self, writes: &mut Vec<Write>, group: GroupId, user: UserId, role: Role, at: i64) {
        if let Some(entry) = self.groups.get_mut(&group) {
            let joined_at = entry.members.get(&user).map_or(at, |(_, joined)| *joined);
            entry.members.insert(user, (role, joined_at));
            self.memberships.entry(user).or_default().insert(group);
            writes.push(Write::GroupMember { group, user, role, at: joined_at });
        }
    }

    fn remove_member(&mut self, writes: &mut Vec<Write>, group: GroupId, user: UserId) {
        if let Some(entry) = self.groups.get_mut(&group) {
            if entry.members.remove(&user).is_some() {
                writes.push(Write::RemoveGroupMember { group, user });
            }
        }
        if let Some(groups) = self.memberships.get_mut(&user) {
            groups.remove(&group);
        }
    }

    pub fn affected(&self, group: GroupId) -> Vec<UserId> {
        self.groups
            .get(&group)
            .map(|group| group.members.keys().copied().collect())
            .unwrap_or_default()
    }

    pub fn create(&mut self, owner: UserId, id: GroupId, name: String, at: i64) -> Result<Vec<Write>, ClientError> {
        if !valid_name(&name) {
            return Err(ClientError::InvalidName);
        }
        if self.of(owner).count() >= MAX_GROUPS {
            return Err(ClientError::GroupFull);
        }
        let mut writes = vec![Write::CreateGroup {
            id,
            name: name.clone(),
            owner,
            at,
        }];
        self.groups.insert(
            id,
            Group {
                name,
                owner,
                members: HashMap::new(),
            },
        );
        self.set_member(&mut writes, id, owner, Role::Owner, at);
        Ok(writes)
    }

    fn group(&self, id: GroupId, user: UserId) -> Result<&Group, ClientError> {
        self.groups
            .get(&id)
            .filter(|group| group.members.contains_key(&user))
            .ok_or(ClientError::UnknownGroup)
    }

    pub fn rename(&mut self, user: UserId, id: GroupId, name: String) -> Result<Vec<Write>, ClientError> {
        if !manages(self.group(id, user)?.role(user)) {
            return Err(ClientError::NotPermitted);
        }
        if !valid_name(&name) {
            return Err(ClientError::InvalidName);
        }
        self.groups.get_mut(&id).expect("checked above").name = name.clone();
        Ok(vec![Write::RenameGroup { id, name }])
    }

    /// `friends` is whether the two are friends, `blocked` whether the target blocked the inviter.
    pub fn invite(&mut self, user: UserId, id: GroupId, target: UserId, friends: bool, blocked: bool, at: i64) -> Result<Vec<Write>, ClientError> {
        let group = self.group(id, user)?;
        if !manages(group.role(user)) {
            return Err(ClientError::NotPermitted);
        }
        if !friends {
            return Err(ClientError::NotFriends);
        }
        if group.members.contains_key(&target) {
            return Ok(Vec::new());
        }
        if group.members.len() >= MAX_MEMBERS {
            return Err(ClientError::GroupFull);
        }
        if blocked {
            return Ok(Vec::new());
        }
        let mut writes = Vec::new();
        self.set_member(&mut writes, id, target, Role::Invited, at);
        Ok(writes)
    }

    pub fn accept(&mut self, user: UserId, id: GroupId, at: i64) -> Result<Vec<Write>, ClientError> {
        if self.group(id, user)?.role(user) != Some(Role::Invited) {
            return Err(ClientError::NoInvite);
        }
        if self.of(user).filter(|(_, group)| group.role(user) != Some(Role::Invited)).count() >= MAX_GROUPS {
            return Err(ClientError::GroupFull);
        }
        let mut writes = Vec::new();
        self.set_member(&mut writes, id, user, Role::Member, at);
        Ok(writes)
    }

    /// Declining an invite and leaving are the same; an owner leaving hands the group on.
    pub fn leave(&mut self, user: UserId, id: GroupId) -> Result<Vec<Write>, ClientError> {
        let group = self.group(id, user)?;
        let mut writes = Vec::new();
        if group.owner != user {
            self.remove_member(&mut writes, id, user);
            return Ok(writes);
        }

        let heir = group
            .members
            .iter()
            .filter(|(member, (role, _))| **member != user && *role != Role::Invited)
            .min_by_key(|(_, (role, joined))| (*role != Role::Admin, *joined))
            .map(|(member, _)| *member);
        match heir {
            Some(heir) => {
                self.remove_member(&mut writes, id, user);
                self.groups.get_mut(&id).expect("checked above").owner = heir;
                writes.push(Write::GroupOwner { id, owner: heir });
                self.set_member(&mut writes, id, heir, Role::Owner, 0);
                Ok(writes)
            }
            None => self.delete(user, id),
        }
    }

    pub fn kick(&mut self, user: UserId, id: GroupId, target: UserId) -> Result<Vec<Write>, ClientError> {
        let group = self.group(id, user)?;
        let (role, target_role) = (group.role(user), group.role(target));
        let allowed = match (role, target_role) {
            (_, None) => return Err(ClientError::UnknownUser),
            (Some(Role::Owner), Some(target_role)) => target_role != Role::Owner,
            (Some(Role::Admin), Some(target_role)) => matches!(target_role, Role::Member | Role::Invited),
            _ => false,
        };
        if !allowed {
            return Err(ClientError::NotPermitted);
        }
        let mut writes = Vec::new();
        self.remove_member(&mut writes, id, target);
        Ok(writes)
    }

    pub fn promote(&mut self, user: UserId, id: GroupId, target: UserId, admin: bool, at: i64) -> Result<Vec<Write>, ClientError> {
        let group = self.group(id, user)?;
        if group.owner != user {
            return Err(ClientError::NotPermitted);
        }
        match group.role(target) {
            Some(Role::Member | Role::Admin) => {}
            None => return Err(ClientError::UnknownUser),
            _ => return Err(ClientError::NotPermitted),
        }
        let mut writes = Vec::new();
        self.set_member(&mut writes, id, target, if admin { Role::Admin } else { Role::Member }, at);
        Ok(writes)
    }

    pub fn delete(&mut self, user: UserId, id: GroupId) -> Result<Vec<Write>, ClientError> {
        if self.group(id, user)?.owner != user {
            return Err(ClientError::NotPermitted);
        }
        let group = self.groups.remove(&id).expect("checked above");
        for member in group.members.keys() {
            if let Some(groups) = self.memberships.get_mut(member) {
                groups.remove(&id);
            }
        }
        Ok(vec![Write::DeleteGroup { id }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: Uuid = Uuid::from_u128(1);
    const ADMIN: Uuid = Uuid::from_u128(2);
    const MEMBER: Uuid = Uuid::from_u128(3);
    const GROUP: Uuid = Uuid::from_u128(100);

    fn group() -> Groups {
        let mut groups = Groups::default();
        groups.create(OWNER, GROUP, "Bedwars".into(), 1).unwrap();
        groups.invite(OWNER, GROUP, ADMIN, true, false, 2).unwrap();
        groups.invite(OWNER, GROUP, MEMBER, true, false, 3).unwrap();
        groups.accept(ADMIN, GROUP, 4).unwrap();
        groups.accept(MEMBER, GROUP, 5).unwrap();
        groups.promote(OWNER, GROUP, ADMIN, true, 6).unwrap();
        groups
    }

    #[test]
    fn names() {
        assert!(valid_name("Bedwars Squad"));
        assert!(!valid_name(""));
        assert!(!valid_name(" padded"));
        assert!(!valid_name(&"x".repeat(25)));
        assert!(!valid_name("§cred"));
    }

    #[test]
    fn roles() {
        let mut groups = group();
        assert_eq!(groups.get(GROUP).unwrap().joined().count(), 3);
        assert_eq!(groups.invite(MEMBER, GROUP, Uuid::from_u128(9), true, false, 7).unwrap_err(), ClientError::NotPermitted);
        assert_eq!(groups.invite(ADMIN, GROUP, Uuid::from_u128(9), false, false, 7).unwrap_err(), ClientError::NotFriends);
        assert_eq!(groups.kick(ADMIN, GROUP, OWNER).unwrap_err(), ClientError::NotPermitted);
        assert_eq!(groups.rename(MEMBER, GROUP, "x".into()).unwrap_err(), ClientError::NotPermitted);
        assert_eq!(groups.delete(ADMIN, GROUP).unwrap_err(), ClientError::NotPermitted);
        assert!(groups.kick(ADMIN, GROUP, MEMBER).is_ok());
        assert_eq!(groups.of(MEMBER).count(), 0);
        assert_eq!(groups.accept(MEMBER, GROUP, 8).unwrap_err(), ClientError::UnknownGroup);
    }

    #[test]
    fn invites_are_not_membership() {
        let mut groups = group();
        let stranger = Uuid::from_u128(9);
        groups.invite(OWNER, GROUP, stranger, true, false, 7).unwrap();
        assert_eq!(groups.get(GROUP).unwrap().role(stranger), Some(Role::Invited));
        assert_eq!(groups.get(GROUP).unwrap().joined().count(), 3);
        assert!(groups.leave(stranger, GROUP).is_ok(), "declining");
        assert_eq!(groups.invite(OWNER, GROUP, stranger, true, true, 8).unwrap(), vec![], "blocked invites vanish");
        assert_eq!(groups.get(GROUP).unwrap().role(stranger), None);
    }

    #[test]
    fn owner_leaving_hands_over() {
        let mut groups = group();
        groups.leave(OWNER, GROUP).unwrap();
        let group = groups.get(GROUP).unwrap();
        assert_eq!(group.owner, ADMIN);
        assert_eq!(group.role(ADMIN), Some(Role::Owner));
        groups.leave(ADMIN, GROUP).unwrap();
        assert_eq!(groups.get(GROUP).unwrap().owner, MEMBER);
        groups.leave(MEMBER, GROUP).unwrap();
        assert!(groups.get(GROUP).is_none(), "the last one out deletes the group");
    }
}
