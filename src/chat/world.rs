use super::UserId;
use crate::ip::canonical;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::IpAddr;
use uuid::Uuid;

/// World ages started this close together, in milliseconds, belong to the same world.
const EPOCH_TOLERANCE: i64 = 5_000;
const DEFAULT_PORT: u16 = 25565;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Relation {
    #[serde(rename = "self")]
    Myself,
    Nearby,
    World,
    Instance,
    Server,
    Elsewhere,
    Offline,
}

impl Relation {
    pub fn shares_position(self) -> bool {
        matches!(self, Relation::Nearby | Relation::World | Relation::Instance)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub uuid: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct Location {
    /// Matches servers; `None` in singleplayer.
    pub key: Option<String>,
    /// Shown to friends and the party; `None` for private addresses.
    pub address: Option<String>,
    pub dimension: Option<String>,
    /// 0 when the server does not tell.
    pub seed: i64,
    /// When the world's age was 0, by the chat server's clock.
    pub epoch: Option<i64>,
    pub player: Option<Player>,
    pub entities: HashSet<UserId>,
    pub tab: HashSet<UserId>,
}

/// The address normalized for display and the key servers are matched by.
pub fn server_key(address: &str, client: IpAddr) -> Option<(Option<String>, String)> {
    let address = address.trim().to_lowercase();
    let (host, port) = match address.strip_prefix('[') {
        Some(rest) => {
            let (host, rest) = rest.split_once(']')?;
            (host.to_owned(), rest.strip_prefix(':').map(str::parse).transpose().ok()?)
        }
        None => match address.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') => (host.to_owned(), Some(port.parse().ok()?)),
            _ => (address.clone(), None),
        },
    };
    let host = host.trim_end_matches('.').to_owned();
    if host.is_empty() {
        return None;
    }
    let port = port.unwrap_or(DEFAULT_PORT);
    let bracketed = if host.contains(':') { format!("[{}]", host) } else { host.clone() };
    let display = if port == DEFAULT_PORT { bracketed.clone() } else { format!("{}:{}", bracketed, port) };

    let private = match host.parse::<IpAddr>().map(canonical) {
        Ok(IpAddr::V4(ip)) => ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified(),
        Ok(IpAddr::V6(ip)) => ip.is_loopback() || ip.is_unspecified() || (ip.segments()[0] & 0xfe00) == 0xfc00 || (ip.segments()[0] & 0xffc0) == 0xfe80,
        Err(_) => host == "localhost" || host.ends_with(".local") || !host.contains('.'),
    };

    let stripped = ["mc.", "play.", "www."]
        .iter()
        .find_map(|prefix| host.strip_prefix(prefix).filter(|rest| rest.contains('.')))
        .unwrap_or(bracketed.as_str());
    let key = format!("{}:{}", stripped, port);
    if private {
        // a LAN address only means the same server behind the same public address
        Some((None, format!("{}@{}", key, canonical(client))))
    } else {
        Some((Some(display), key))
    }
}

/// How `b` relates to `a`, both being someone's location if in game.
pub fn relation(a: Option<&Location>, a_user: UserId, b: Option<&Location>, b_user: UserId, unreliable: &HashSet<String>) -> Relation {
    if a_user == b_user {
        return Relation::Myself;
    }
    let (Some(a), Some(b)) = (a, b) else { return Relation::Offline };
    if a.entities.contains(&b_user) || b.entities.contains(&a_user) {
        return Relation::Nearby;
    }
    let (Some(key), Some(other)) = (&a.key, &b.key) else { return Relation::Elsewhere };
    if key != other {
        return Relation::Elsewhere;
    }

    let same_age = matches!((a.epoch, b.epoch), (Some(a), Some(b)) if (a - b).abs() <= EPOCH_TOLERANCE);
    if same_age {
        if a.dimension == b.dimension {
            let seeds = a.seed != 0 && a.seed == b.seed && !unreliable.contains(key);
            let tab = a.tab.contains(&b_user) || b.tab.contains(&a_user);
            if seeds || tab {
                return Relation::World;
            }
        } else if a.seed == b.seed || a.seed == 0 || b.seed == 0 {
            return Relation::Instance;
        }
    }
    Relation::Server
}

/// Seeds differing for players who see each other: the server hides or randomizes them.
pub fn contradicts_seed(a: &Location, a_user: UserId, b: &Location, b_user: UserId) -> bool {
    (a.entities.contains(&b_user) || b.entities.contains(&a_user))
        && a.key.is_some()
        && a.key == b.key
        && a.seed != 0
        && b.seed != 0
        && a.seed != b.seed
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Uuid = Uuid::from_u128(1);
    const B: Uuid = Uuid::from_u128(2);

    fn client() -> IpAddr {
        "203.0.113.7".parse().unwrap()
    }

    fn at(server: &str, dimension: &str, seed: i64, epoch: Option<i64>) -> Location {
        let (address, key) = server_key(server, client()).unwrap();
        Location {
            key: Some(key),
            address,
            dimension: Some(dimension.into()),
            seed,
            epoch,
            ..Location::default()
        }
    }

    fn rel(a: &Location, b: &Location) -> Relation {
        relation(Some(a), A, Some(b), B, &HashSet::new())
    }

    #[test]
    fn keys() {
        let key = |address: &str| server_key(address, client()).unwrap();
        assert_eq!(key("Hypixel.NET."), (Some("hypixel.net".into()), "hypixel.net:25565".into()));
        assert_eq!(key("mc.hypixel.net:25565"), (Some("mc.hypixel.net".into()), "hypixel.net:25565".into()));
        assert_eq!(key("play.example.org:25566").1, "example.org:25566");
        assert_eq!(key("mc.com").1, "mc.com:25565", "a prefix is only stripped when a domain remains");
        assert_eq!(key("[2001:db8::1]:25570"), (Some("[2001:db8::1]:25570".into()), "[2001:db8::1]:25570".into()));
        assert_eq!(key("192.168.1.20"), (None, "192.168.1.20:25565@203.0.113.7".into()));
        assert_eq!(key("localhost:25570").0, None);
        assert!(server_key("", client()).is_none());
        assert!(server_key("host:notaport", client()).is_none());
    }

    #[test]
    fn relations() {
        let lobby = at("hypixel.net", "minecraft:overworld", 0, Some(1_000_000));
        assert_eq!(relation(Some(&lobby), A, Some(&lobby), A, &HashSet::new()), Relation::Myself);
        assert_eq!(relation(Some(&lobby), A, None, B, &HashSet::new()), Relation::Offline);

        // proxy backends: same address, different worlds
        let survival = at("example.org", "minecraft:overworld", 42, Some(1_000_000));
        let creative = at("example.org", "minecraft:overworld", 7, Some(3_000_000));
        assert_eq!(rel(&survival, &creative), Relation::Server);
        assert_eq!(rel(&survival, &survival.clone()), Relation::World);

        // the nether of the same save
        let nether = at("example.org", "minecraft:the_nether", 42, Some(1_002_000));
        assert_eq!(rel(&survival, &nether), Relation::Instance);

        // within the tolerance, but lagging
        let lagging = at("example.org", "minecraft:overworld", 42, Some(1_004_900));
        assert_eq!(rel(&survival, &lagging), Relation::World);
        let drifted = at("example.org", "minecraft:overworld", 42, Some(1_005_100));
        assert_eq!(rel(&survival, &drifted), Relation::Server);

        // seedless networks need the tab list
        let mut a = lobby.clone();
        let mut b = lobby.clone();
        assert_eq!(rel(&a, &b), Relation::Server);
        b.tab.insert(A);
        assert_eq!(rel(&a, &b), Relation::World);
        b.tab.clear();
        a.entities.insert(B);
        assert_eq!(rel(&a, &b), Relation::Nearby);

        // frozen time
        let frozen = at("example.org", "minecraft:overworld", 42, None);
        assert_eq!(rel(&survival, &frozen), Relation::Server);

        assert_eq!(rel(&survival, &at("other.net", "minecraft:overworld", 42, Some(1_000_000))), Relation::Elsewhere);
        let singleplayer = Location::default();
        assert_eq!(rel(&survival, &singleplayer), Relation::Elsewhere);
    }

    #[test]
    fn unreliable_seeds() {
        let mut a = at("example.org", "minecraft:overworld", 42, Some(1_000_000));
        let b = at("example.org", "minecraft:overworld", 42, Some(1_000_000));
        let unreliable = HashSet::from([a.key.clone().unwrap()]);
        assert_eq!(relation(Some(&a), A, Some(&b), B, &unreliable), Relation::Server);

        let mut c = b.clone();
        c.seed = 99;
        a.entities.insert(B);
        assert!(contradicts_seed(&a, A, &c, B));
        assert!(!contradicts_seed(&a, A, &b, B));
    }
}
