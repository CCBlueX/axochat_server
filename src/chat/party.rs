use super::UserId;
use crate::error::ClientError;

use serde::Serialize;
use std::collections::HashMap;
use uuid::Uuid;

pub type PartyId = Uuid;

pub const MAX_MEMBERS: usize = 8;
pub const INVITE_TIME: i64 = 60_000;
/// Members who go offline stay this long.
pub const OFFLINE_GRACE: i64 = 5 * 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PartyRole {
    Leader,
    Admin,
    Member,
}

#[derive(Debug, Clone)]
pub struct Member {
    pub user: UserId,
    pub role: PartyRole,
    pub joined: i64,
    pub muted: bool,
    pub offline_since: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Invite {
    pub user: UserId,
    pub expires: i64,
}

#[derive(Debug, Clone)]
pub struct Party {
    pub id: PartyId,
    pub members: Vec<Member>,
    pub locked: bool,
    pub pvp: bool,
    pub invites: Vec<Invite>,
}

impl Party {
    pub fn leader(&self) -> UserId {
        self.members
            .iter()
            .find(|member| member.role == PartyRole::Leader)
            .map(|member| member.user)
            .expect("a party has a leader")
    }

    pub fn member(&self, user: UserId) -> Option<&Member> {
        self.members.iter().find(|member| member.user == user)
    }

    fn member_mut(&mut self, user: UserId) -> Option<&mut Member> {
        self.members.iter_mut().find(|member| member.user == user)
    }

    pub fn users(&self) -> impl Iterator<Item = UserId> + '_ {
        self.members.iter().map(|member| member.user)
    }

    fn role(&self, user: UserId) -> Option<PartyRole> {
        self.member(user).map(|member| member.role)
    }

    fn require(&self, user: UserId, leader_only: bool) -> Result<(), ClientError> {
        match self.role(user) {
            Some(PartyRole::Leader) => Ok(()),
            Some(PartyRole::Admin) if !leader_only => Ok(()),
            _ => Err(ClientError::NotPermitted),
        }
    }

    /// The oldest admin, else the oldest member, takes over from a leaving leader.
    fn hand_over(&mut self) {
        let heir = self
            .members
            .iter()
            .filter(|member| member.role != PartyRole::Leader)
            .min_by_key(|member| (member.role != PartyRole::Admin, member.offline_since.is_some(), member.joined))
            .map(|member| member.user);
        if let Some(heir) = heir.and_then(|heir| self.member_mut(heir)) {
            heir.role = PartyRole::Leader;
        }
    }

    fn remove(&mut self, user: UserId) {
        let was_leader = self.role(user) == Some(PartyRole::Leader);
        self.members.retain(|member| member.user != user);
        if was_leader && !self.members.is_empty() {
            self.hand_over();
        }
    }
}

#[derive(Default)]
pub struct Parties {
    parties: HashMap<PartyId, Party>,
    member_of: HashMap<UserId, PartyId>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Change {
    pub party: Option<PartyId>,
    pub removed: Vec<UserId>,
    pub disbanded: bool,
}

impl Parties {
    pub fn of(&self, user: UserId) -> Option<&Party> {
        self.parties.get(self.member_of.get(&user)?)
    }

    pub fn get(&self, id: PartyId) -> Option<&Party> {
        self.parties.get(&id)
    }

    pub fn all(&self) -> impl Iterator<Item = &Party> {
        self.parties.values()
    }

    fn own_mut(&mut self, user: UserId) -> Result<&mut Party, ClientError> {
        let id = self.member_of.get(&user).ok_or(ClientError::NotInParty)?;
        Ok(self.parties.get_mut(id).expect("memberships point at parties"))
    }

    /// Founds a party first if `user` has none. Without a target, everything happens but the invite,
    /// so it looks the same to `user`. Returns whether the target is to be told; a pending invite
    /// is not repeated.
    pub fn invite(&mut self, user: UserId, target: UserId, new_id: PartyId, now: i64) -> Result<PartyId, ClientError> {
        if user == target {
            return Err(ClientError::NotPermitted);
        }
        if !self.member_of.contains_key(&user) {
            self.parties.insert(
                new_id,
                Party {
                    id: new_id,
                    members: vec![Member {
                        user,
                        role: PartyRole::Leader,
                        joined: now,
                        muted: false,
                        offline_since: None,
                    }],
                    locked: false,
                    pvp: false,
                    invites: Vec::new(),
                },
            );
            self.member_of.insert(user, new_id);
        }

        let party = self.own_mut(user)?;
        party.require(user, false)?;
        if party.locked {
            return Err(ClientError::PartyLocked);
        }
        if party.member(target).is_some() {
            return Err(ClientError::AlreadyInParty);
        }
        if party.members.len() >= MAX_MEMBERS {
            return Err(ClientError::PartyFull);
        }
        party.invites.retain(|invite| invite.user != target);
        party.invites.push(Invite {
            user: target,
            expires: now + INVITE_TIME,
        });
        Ok(party.id)
    }

    pub fn accept(&mut self, user: UserId, id: PartyId, now: i64) -> Result<Change, ClientError> {
        if self.member_of.contains_key(&user) {
            return Err(ClientError::AlreadyInParty);
        }
        let party = self.parties.get_mut(&id).ok_or(ClientError::NoInvite)?;
        if !party.invites.iter().any(|invite| invite.user == user && invite.expires > now) {
            return Err(ClientError::NoInvite);
        }
        if party.locked {
            return Err(ClientError::PartyLocked);
        }
        if party.members.len() >= MAX_MEMBERS {
            return Err(ClientError::PartyFull);
        }
        party.invites.retain(|invite| invite.user != user);
        party.members.push(Member {
            user,
            role: PartyRole::Member,
            joined: now,
            muted: false,
            offline_since: None,
        });
        self.member_of.insert(user, id);
        Ok(Change {
            party: Some(id),
            ..Change::default()
        })
    }

    pub fn decline(&mut self, user: UserId, id: PartyId) -> Result<(), ClientError> {
        let party = self.parties.get_mut(&id).ok_or(ClientError::NoInvite)?;
        let before = party.invites.len();
        party.invites.retain(|invite| invite.user != user);
        if party.invites.len() == before {
            return Err(ClientError::NoInvite);
        }
        Ok(())
    }

    fn remove(&mut self, id: PartyId, user: UserId) -> Change {
        let party = self.parties.get_mut(&id).expect("checked by the caller");
        party.remove(user);
        self.member_of.remove(&user);
        let mut change = Change {
            party: Some(id),
            removed: vec![user],
            disbanded: false,
        };
        if party.members.is_empty() {
            self.parties.remove(&id);
            change.disbanded = true;
        }
        change
    }

    pub fn leave(&mut self, user: UserId) -> Result<Change, ClientError> {
        let id = self.own_mut(user)?.id;
        Ok(self.remove(id, user))
    }

    pub fn kick(&mut self, user: UserId, target: UserId) -> Result<Change, ClientError> {
        let party = self.own_mut(user)?;
        party.require(user, false)?;
        let allowed = match (party.role(user), party.role(target)) {
            (_, None) => return Err(ClientError::NotInParty),
            (Some(PartyRole::Leader), Some(role)) => role != PartyRole::Leader,
            (Some(PartyRole::Admin), Some(role)) => role == PartyRole::Member,
            _ => false,
        };
        if !allowed {
            return Err(ClientError::NotPermitted);
        }
        let id = party.id;
        Ok(self.remove(id, target))
    }

    pub fn promote(&mut self, user: UserId, target: UserId, admin: bool) -> Result<PartyId, ClientError> {
        let party = self.own_mut(user)?;
        party.require(user, true)?;
        let member = party.member_mut(target).ok_or(ClientError::NotInParty)?;
        if member.role == PartyRole::Leader {
            return Err(ClientError::NotPermitted);
        }
        member.role = if admin { PartyRole::Admin } else { PartyRole::Member };
        Ok(party.id)
    }

    pub fn transfer(&mut self, user: UserId, target: UserId) -> Result<PartyId, ClientError> {
        let party = self.own_mut(user)?;
        party.require(user, true)?;
        if user == target {
            return Ok(party.id);
        }
        party.member(target).ok_or(ClientError::NotInParty)?;
        party.member_mut(user).expect("checked above").role = PartyRole::Admin;
        party.member_mut(target).expect("checked above").role = PartyRole::Leader;
        Ok(party.id)
    }

    pub fn lock(&mut self, user: UserId, locked: bool) -> Result<PartyId, ClientError> {
        let party = self.own_mut(user)?;
        party.require(user, true)?;
        party.locked = locked;
        if locked {
            party.invites.clear();
        }
        Ok(party.id)
    }

    pub fn mute(&mut self, user: UserId, target: UserId, muted: bool) -> Result<PartyId, ClientError> {
        let party = self.own_mut(user)?;
        party.require(user, true)?;
        let member = party.member_mut(target).ok_or(ClientError::NotInParty)?;
        if member.role == PartyRole::Leader {
            return Err(ClientError::NotPermitted);
        }
        member.muted = muted;
        Ok(party.id)
    }

    pub fn pvp(&mut self, user: UserId, enabled: bool) -> Result<PartyId, ClientError> {
        let party = self.own_mut(user)?;
        party.require(user, true)?;
        party.pvp = enabled;
        Ok(party.id)
    }

    pub fn disband(&mut self, user: UserId) -> Result<Change, ClientError> {
        let party = self.own_mut(user)?;
        party.require(user, true)?;
        let id = party.id;
        let party = self.parties.remove(&id).expect("checked above");
        for member in party.users() {
            self.member_of.remove(&member);
        }
        Ok(Change {
            party: Some(id),
            removed: party.users().collect(),
            disbanded: true,
        })
    }

    pub fn set_online(&mut self, user: UserId, online: bool, now: i64) -> Option<PartyId> {
        let id = *self.member_of.get(&user)?;
        let member = self.parties.get_mut(&id)?.member_mut(user)?;
        member.offline_since = if online { None } else { member.offline_since.or(Some(now)) };
        Some(id)
    }

    pub fn expire(&mut self, now: i64) -> Vec<Change> {
        let mut gone = Vec::new();
        for party in self.parties.values_mut() {
            party.invites.retain(|invite| invite.expires > now);
            for member in &party.members {
                if member.offline_since.is_some_and(|since| now - since >= OFFLINE_GRACE) {
                    gone.push((party.id, member.user));
                }
            }
        }
        gone.into_iter().map(|(id, user)| self.remove(id, user)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEADER: Uuid = Uuid::from_u128(1);
    const ADMIN: Uuid = Uuid::from_u128(2);
    const MEMBER: Uuid = Uuid::from_u128(3);
    const PARTY: Uuid = Uuid::from_u128(100);

    fn party() -> Parties {
        let mut parties = Parties::default();
        parties.invite(LEADER, ADMIN, PARTY, 0).unwrap();
        parties.invite(LEADER, MEMBER, Uuid::from_u128(101), 0).unwrap();
        parties.accept(ADMIN, PARTY, 1).unwrap();
        parties.accept(MEMBER, PARTY, 2).unwrap();
        parties.promote(LEADER, ADMIN, true).unwrap();
        parties
    }

    #[test]
    fn invites() {
        let mut parties = party();
        let guest = Uuid::from_u128(9);
        assert_eq!(parties.invite(MEMBER, guest, PARTY, 3).unwrap_err(), ClientError::NotPermitted);
        assert_eq!(parties.invite(ADMIN, guest, PARTY, 3).unwrap(), PARTY);
        assert_eq!(parties.accept(guest, PARTY, 3 + INVITE_TIME).unwrap_err(), ClientError::NoInvite, "expired");
        parties.invite(ADMIN, guest, PARTY, 4).unwrap();
        parties.lock(LEADER, true).unwrap();
        assert_eq!(parties.accept(guest, PARTY, 5).unwrap_err(), ClientError::NoInvite, "locking drops invites");
        assert_eq!(parties.invite(LEADER, guest, PARTY, 6).unwrap_err(), ClientError::PartyLocked);
        assert_eq!(parties.accept(MEMBER, PARTY, 6).unwrap_err(), ClientError::AlreadyInParty);
    }

    #[test]
    fn full() {
        let mut parties = party();
        parties.lock(LEADER, false).unwrap();
        for i in 10..15 {
            let user = Uuid::from_u128(i);
            parties.invite(LEADER, user, PARTY, 0).unwrap();
            parties.accept(user, PARTY, 1).unwrap();
        }
        assert_eq!(parties.of(LEADER).unwrap().members.len(), MAX_MEMBERS);
        assert_eq!(parties.invite(LEADER, Uuid::from_u128(20), PARTY, 0).unwrap_err(), ClientError::PartyFull);
    }

    #[test]
    fn roles() {
        let mut parties = party();
        assert_eq!(parties.kick(ADMIN, LEADER).unwrap_err(), ClientError::NotPermitted);
        assert_eq!(parties.pvp(ADMIN, true).unwrap_err(), ClientError::NotPermitted);
        assert_eq!(parties.mute(LEADER, MEMBER, true).unwrap(), PARTY);
        assert!(parties.of(MEMBER).unwrap().member(MEMBER).unwrap().muted);
        parties.transfer(LEADER, MEMBER).unwrap();
        assert_eq!(parties.of(LEADER).unwrap().leader(), MEMBER);
        let change = parties.kick(MEMBER, LEADER).unwrap();
        assert_eq!(change.removed, vec![LEADER]);
        assert!(parties.of(LEADER).is_none());
    }

    #[test]
    fn leaving_and_expiry() {
        let mut parties = party();
        parties.leave(LEADER).unwrap();
        assert_eq!(parties.of(MEMBER).unwrap().leader(), ADMIN, "admins inherit first");

        parties.set_online(MEMBER, false, 10);
        assert!(parties.expire(10 + OFFLINE_GRACE - 1).is_empty());
        assert_eq!(parties.expire(10 + OFFLINE_GRACE)[0].removed, vec![MEMBER]);
        parties.set_online(ADMIN, false, 20);
        parties.set_online(ADMIN, true, 30);
        assert!(parties.expire(20 + OFFLINE_GRACE).is_empty(), "coming back resets the grace");

        let change = parties.leave(ADMIN).unwrap();
        assert!(change.disbanded);
        assert_eq!(parties.all().count(), 0);
    }
}
