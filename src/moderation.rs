use crate::entity::punishment::{self, Kind};
use crate::ip::Cidr;

use std::net::IpAddr;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Punishment {
    pub id: Uuid,
    pub kind: Kind,
    pub user: Option<Uuid>,
    pub ip: Option<Cidr>,
    pub reason: String,
    pub issued_by: Option<Uuid>,
    pub created_at: i64,
    pub expires_at: Option<i64>,
}

impl Punishment {
    pub fn from_model(model: punishment::Model) -> Punishment {
        Punishment {
            id: model.id,
            kind: model.kind,
            user: model.user_id,
            ip: model.ip.and_then(|ip| ip.parse().ok()),
            reason: model.reason,
            issued_by: model.issued_by,
            created_at: model.created_at,
            expires_at: model.expires_at,
        }
    }

    pub fn to_model(&self) -> punishment::Model {
        punishment::Model {
            id: self.id,
            kind: self.kind,
            user_id: self.user,
            ip: self.ip.map(|ip| ip.to_string()),
            reason: self.reason.clone(),
            issued_by: self.issued_by,
            created_at: self.created_at,
            expires_at: self.expires_at,
            revoked_at: None,
            revoked_by: None,
        }
    }

    pub fn is_active(&self, now: i64) -> bool {
        self.expires_at.is_none_or(|expires| expires > now)
    }

    pub fn covers(&self, user: Option<Uuid>, ip: Option<IpAddr>) -> bool {
        (user.is_some() && self.user == user)
            || ip.is_some_and(|ip| self.ip.is_some_and(|range| range.contains(ip)))
    }
}

#[derive(Default)]
pub struct Moderation {
    punishments: Vec<Punishment>,
}

impl Moderation {
    pub fn new(punishments: Vec<Punishment>) -> Moderation {
        Moderation { punishments }
    }

    /// The longest-lasting punishment of `kind` covering the user or the address; a ban also mutes.
    pub fn find(&self, kind: Kind, user: Option<Uuid>, ip: Option<IpAddr>, now: i64) -> Option<&Punishment> {
        self.punishments
            .iter()
            .filter(|p| p.is_active(now) && (p.kind == kind || p.kind == Kind::Ban) && p.covers(user, ip))
            .max_by_key(|p| p.expires_at.unwrap_or(i64::MAX))
    }

    pub fn of_user(&self, user: Uuid, now: i64) -> Vec<&Punishment> {
        self.punishments
            .iter()
            .filter(|p| p.is_active(now) && p.user == Some(user))
            .collect()
    }

    pub fn add(&mut self, punishment: Punishment) {
        self.punishments.push(punishment);
    }

    pub fn revoke(&mut self, user: Option<Uuid>, ip: Option<Cidr>) -> Vec<Uuid> {
        let mut revoked = Vec::new();
        self.punishments.retain(|p| {
            let matches = (user.is_some() && p.user == user) || (ip.is_some() && p.ip == ip);
            if matches {
                revoked.push(p.id);
            }
            !matches
        });
        revoked
    }

    pub fn prune(&mut self, now: i64) {
        self.punishments.retain(|p| p.is_active(now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn punishment(kind: Kind, user: Option<Uuid>, ip: Option<&str>, expires_at: Option<i64>) -> Punishment {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Punishment {
            id: Uuid::from_u128(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128),
            kind,
            user,
            ip: ip.map(|ip| ip.parse().unwrap()),
            reason: String::new(),
            issued_by: None,
            created_at: 0,
            expires_at,
        }
    }

    #[test]
    fn find() {
        let (alice, bob) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let home: IpAddr = "203.0.113.7".parse().unwrap();
        let v6: IpAddr = "2001:db8:1:2::1".parse().unwrap();
        let mut moderation = Moderation::default();
        moderation.add(punishment(Kind::Mute, Some(alice), None, Some(100)));
        moderation.add(punishment(Kind::Ban, None, Some("2001:db8:1:2::/64"), None));

        assert!(moderation.find(Kind::Mute, Some(alice), None, 50).is_some());
        assert!(moderation.find(Kind::Mute, Some(alice), None, 100).is_none(), "expired");
        assert!(moderation.find(Kind::Ban, Some(alice), None, 50).is_none(), "a mute is no ban");
        assert!(moderation.find(Kind::Mute, Some(bob), None, 50).is_none());
        assert!(moderation.find(Kind::Ban, Some(bob), Some(v6), 50).is_some());
        assert!(moderation.find(Kind::Mute, Some(bob), Some(v6), 50).is_some(), "a ban also mutes");
        assert!(moderation.find(Kind::Ban, None, Some(home), 50).is_none());

        moderation.add(punishment(Kind::Ban, Some(bob), Some("203.0.113.7/32"), Some(200)));
        assert!(moderation.find(Kind::Ban, None, Some(home), 50).is_some(), "ban with include_ip");
        assert_eq!(moderation.revoke(Some(bob), None).len(), 1);
        assert!(moderation.find(Kind::Ban, None, Some(home), 50).is_none(), "pardon lifts the ip too");

        moderation.prune(150);
        assert_eq!(moderation.punishments.len(), 1);
    }
}
