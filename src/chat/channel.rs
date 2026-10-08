use super::UserId;

use std::fmt;
use uuid::Uuid;

/// Direct messages address the user by id or name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    Global,
    Server,
    Party,
    Group(Uuid),
    User(String),
}

impl Channel {
    pub fn parse(channel: &str) -> Option<Channel> {
        Some(match channel.split_once('/') {
            None if channel == "global" => Channel::Global,
            None if channel == "server" => Channel::Server,
            None if channel == "party" => Channel::Party,
            Some(("group", id)) => Channel::Group(Uuid::parse_str(id).ok()?),
            Some(("user", user)) if !user.is_empty() => Channel::User(user.to_owned()),
            _ => return None,
        })
    }

    pub fn direct(user: UserId) -> Channel {
        Channel::User(user.to_string())
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Channel::Global => f.write_str("global"),
            Channel::Server => f.write_str("server"),
            Channel::Party => f.write_str("party"),
            Channel::Group(id) => write!(f, "group/{}", id),
            Channel::User(user) => write!(f, "user/{}", user),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Channel;
    use uuid::Uuid;

    #[test]
    fn parse() {
        assert_eq!(Channel::parse("global"), Some(Channel::Global));
        assert_eq!(Channel::parse("party"), Some(Channel::Party));
        assert_eq!(Channel::parse("user/Notch"), Some(Channel::User("Notch".into())));
        let id = Uuid::from_u128(7);
        assert_eq!(Channel::parse(&format!("group/{}", id)), Some(Channel::Group(id)));
        assert_eq!(Channel::Group(id).to_string(), format!("group/{}", id));
        for invalid in ["", "user/", "group/nope", "global/x", "lobby"] {
            assert_eq!(Channel::parse(invalid), None, "{}", invalid);
        }
    }
}
