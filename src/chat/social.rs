use super::UserId;
use crate::entity::relation::{self, Kind};
use crate::error::ClientError;
use crate::store::Write;

use std::collections::{HashMap, HashSet};

pub const MAX_FRIENDS: usize = 200;
const MAX_PENDING: usize = 100;

#[derive(Default)]
pub struct Social {
    outgoing: HashMap<UserId, HashMap<UserId, (Kind, i64)>>,
    incoming: HashMap<UserId, HashSet<UserId>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Requested {
    /// The other side had asked already, so they are friends now.
    Friends,
    Pending,
}

impl Social {
    pub fn new(relations: Vec<relation::Model>) -> Social {
        let mut social = Social::default();
        for relation in relations {
            social.set(relation.user_id, relation.target_id, relation.kind, relation.created_at);
        }
        social
    }

    pub fn get(&self, user: UserId, target: UserId) -> Option<Kind> {
        self.outgoing.get(&user)?.get(&target).map(|(kind, _)| *kind)
    }

    pub fn has_blocked(&self, user: UserId, target: UserId) -> bool {
        self.get(user, target) == Some(Kind::Block)
    }

    pub fn are_friends(&self, user: UserId, target: UserId) -> bool {
        self.get(user, target) == Some(Kind::Friend)
    }

    fn of_kind(&self, user: UserId, kind: Kind) -> impl Iterator<Item = (UserId, i64)> + '_ {
        self.outgoing
            .get(&user)
            .into_iter()
            .flatten()
            .filter(move |(_, (k, _))| *k == kind)
            .map(|(target, (_, since))| (*target, *since))
    }

    pub fn friends(&self, user: UserId) -> impl Iterator<Item = (UserId, i64)> + '_ {
        self.of_kind(user, Kind::Friend)
    }

    pub fn outgoing_requests(&self, user: UserId) -> impl Iterator<Item = UserId> + '_ {
        self.of_kind(user, Kind::Request).map(|(target, _)| target)
    }

    /// Requests towards `user`, without those from users `user` blocked.
    pub fn incoming_requests(&self, user: UserId) -> impl Iterator<Item = UserId> + '_ {
        self.incoming
            .get(&user)
            .into_iter()
            .flatten()
            .copied()
            .filter(move |from| self.get(*from, user) == Some(Kind::Request) && !self.has_blocked(user, *from))
    }

    pub fn blocked(&self, user: UserId) -> impl Iterator<Item = UserId> + '_ {
        self.of_kind(user, Kind::Block).map(|(target, _)| target)
    }

    fn set(&mut self, user: UserId, target: UserId, kind: Kind, at: i64) {
        self.outgoing.entry(user).or_default().insert(target, (kind, at));
        self.incoming.entry(target).or_default().insert(user);
    }

    fn unset(&mut self, user: UserId, target: UserId) -> bool {
        let removed = self
            .outgoing
            .get_mut(&user)
            .is_some_and(|targets| targets.remove(&target).is_some());
        if let Some(sources) = self.incoming.get_mut(&target) {
            sources.remove(&user);
        }
        removed
    }

    fn write_set(&mut self, writes: &mut Vec<Write>, user: UserId, target: UserId, kind: Kind, at: i64) {
        self.set(user, target, kind, at);
        writes.push(Write::SetRelation { user, target, kind, at });
    }

    fn write_unset(&mut self, writes: &mut Vec<Write>, user: UserId, target: UserId) {
        if self.unset(user, target) {
            writes.push(Write::RemoveRelation { user, target });
        }
    }

    /// `accepts` is whether `target` takes friend requests at all.
    pub fn request(&mut self, user: UserId, target: UserId, accepts: bool, at: i64) -> Result<(Requested, Vec<Write>), ClientError> {
        if user == target || self.has_blocked(user, target) {
            return Err(ClientError::NotPermitted);
        }
        match self.get(user, target) {
            Some(Kind::Friend) => return Err(ClientError::AlreadyFriends),
            Some(Kind::Request) => return Ok((Requested::Pending, Vec::new())),
            _ => {}
        }
        if self.friends(user).count() >= MAX_FRIENDS || self.outgoing_requests(user).count() >= MAX_PENDING {
            return Err(ClientError::NotPermitted);
        }

        let mut writes = Vec::new();
        if self.get(target, user) == Some(Kind::Request) {
            self.befriend(&mut writes, user, target, at);
            return Ok((Requested::Friends, writes));
        }
        // a request towards someone who blocked the sender is kept, but never shown to them;
        // one towards someone who takes none vanishes, like one towards a name nobody has
        if !accepts && !self.has_blocked(target, user) {
            return Ok((Requested::Pending, Vec::new()));
        }
        self.write_set(&mut writes, user, target, Kind::Request, at);
        Ok((Requested::Pending, writes))
    }

    pub fn accept(&mut self, user: UserId, from: UserId, at: i64) -> Result<Vec<Write>, ClientError> {
        if self.get(from, user) != Some(Kind::Request) || self.has_blocked(user, from) {
            return Err(ClientError::NoInvite);
        }
        if self.friends(user).count() >= MAX_FRIENDS {
            return Err(ClientError::NotPermitted);
        }
        let mut writes = Vec::new();
        self.befriend(&mut writes, user, from, at);
        Ok(writes)
    }

    fn befriend(&mut self, writes: &mut Vec<Write>, user: UserId, target: UserId, at: i64) {
        self.write_set(writes, user, target, Kind::Friend, at);
        self.write_set(writes, target, user, Kind::Friend, at);
    }

    pub fn decline(&mut self, user: UserId, from: UserId) -> Result<Vec<Write>, ClientError> {
        if self.get(from, user) != Some(Kind::Request) {
            return Err(ClientError::NoInvite);
        }
        let mut writes = Vec::new();
        self.write_unset(&mut writes, from, user);
        Ok(writes)
    }

    /// Ends a friendship, or withdraws a request.
    pub fn remove(&mut self, user: UserId, target: UserId) -> Result<Vec<Write>, ClientError> {
        let mut writes = Vec::new();
        match self.get(user, target) {
            Some(Kind::Friend) => {
                self.write_unset(&mut writes, user, target);
                self.write_unset(&mut writes, target, user);
            }
            Some(Kind::Request) => self.write_unset(&mut writes, user, target),
            _ => return Err(ClientError::NotFriends),
        }
        Ok(writes)
    }

    pub fn block(&mut self, user: UserId, target: UserId, blocked: bool, at: i64) -> Result<Vec<Write>, ClientError> {
        if user == target {
            return Err(ClientError::NotPermitted);
        }
        let mut writes = Vec::new();
        if blocked {
            if self.has_blocked(user, target) {
                return Ok(writes);
            }
            self.write_unset(&mut writes, user, target);
            if self.get(target, user) != Some(Kind::Block) {
                self.write_unset(&mut writes, target, user);
            }
            self.write_set(&mut writes, user, target, Kind::Block, at);
        } else if self.has_blocked(user, target) {
            self.write_unset(&mut writes, user, target);
        }
        Ok(writes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    const A: Uuid = Uuid::from_u128(1);
    const B: Uuid = Uuid::from_u128(2);
    const C: Uuid = Uuid::from_u128(3);

    #[test]
    fn friendship() {
        let mut social = Social::default();
        assert_eq!(social.request(A, B, true, 1).unwrap().0, Requested::Pending);
        let (requested, writes) = social.request(A, B, true, 1).unwrap();
        assert!(requested == Requested::Pending && writes.is_empty(), "idempotent");
        assert_eq!(social.incoming_requests(B).collect::<Vec<_>>(), vec![A]);
        assert_eq!(social.accept(B, A, 2).unwrap().len(), 2);
        assert!(social.are_friends(A, B) && social.are_friends(B, A));
        assert_eq!(social.incoming_requests(B).count(), 0);
        assert_eq!(social.request(B, A, true, 3).unwrap_err(), ClientError::AlreadyFriends);
        assert_eq!(social.remove(B, A).unwrap().len(), 2);
        assert!(!social.are_friends(A, B));
        assert_eq!(social.remove(B, A).unwrap_err(), ClientError::NotFriends);
    }

    #[test]
    fn crossing_requests_befriend() {
        let mut social = Social::default();
        social.request(A, B, true, 1).unwrap();
        assert_eq!(social.request(B, A, false, 2).unwrap().0, Requested::Friends);
        assert!(social.are_friends(A, B));
    }

    #[test]
    fn requests_disabled_and_declined() {
        let mut social = Social::default();
        assert!(social.request(A, B, false, 1).unwrap().1.is_empty());
        assert_eq!(social.incoming_requests(B).count(), 0);
        social.request(A, C, true, 1).unwrap();
        assert_eq!(social.decline(C, A).unwrap().len(), 1);
        assert_eq!(social.decline(C, A).unwrap_err(), ClientError::NoInvite);
        assert_eq!(social.accept(C, A, 2).unwrap_err(), ClientError::NoInvite);
    }

    #[test]
    fn blocking() {
        let mut social = Social::default();
        social.request(A, B, true, 1).unwrap();
        social.accept(B, A, 2).unwrap();
        social.block(B, A, true, 3).unwrap();
        assert!(!social.are_friends(A, B) && !social.are_friends(B, A));
        assert!(social.has_blocked(B, A));

        // requests towards a blocker look pending to the sender but never reach the blocker
        assert_eq!(social.request(A, B, false, 4).unwrap().0, Requested::Pending);
        assert_eq!(social.incoming_requests(B).count(), 0);
        assert_eq!(social.accept(B, A, 5).unwrap_err(), ClientError::NoInvite);
        assert_eq!(social.request(B, A, true, 5).unwrap_err(), ClientError::NotPermitted);

        social.block(B, A, false, 6).unwrap();
        assert!(!social.has_blocked(B, A));
        assert_eq!(social.incoming_requests(B).collect::<Vec<_>>(), vec![A]);
    }
}
