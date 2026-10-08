use super::party::PartyRole;
use super::world::{Player, Relation};
use super::{Frame, Identity, Kind};
use crate::entity::chat_group_member::Role;
use crate::entity::punishment;
use crate::error::ClientError;
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL: u32 = 2;
/// Beyond the largest world border; also rules out NaN and infinity.
const MAX_COORDINATE: f64 = 30_000_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Protocol {
    V1,
    V2,
}

/// A clientbound packet
#[derive(Serialize, Clone)]
#[serde(tag = "m", content = "c")]
pub enum ClientPacket {
    MojangInfo {
        session_hash: String,
    },
    Message {
        author_info: UserInfo,
        content: String,
    },
    PrivateMessage {
        author_info: UserInfo,
        content: String,
    },
    UserCount {
        connections: u32,
        logged_in: u32,
    },
    Success {
        reason: SuccessReason,
    },
    Error {
        message: ClientError,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    Hello {
        protocol: u32,
    },
    Punished {
        kind: punishment::Kind,
        reason: String,
        expires: Option<i64>,
    },
    Punishments {
        user: UserRef,
        punishments: Vec<PunishmentView>,
    },
    Welcome {
        user: Author,
        staff: bool,
    },
    Settings(SettingsView),
    Friends {
        friends: Vec<FriendView>,
        incoming: Vec<UserRef>,
        outgoing: Vec<UserRef>,
    },
    Presence {
        user: Uuid,
        online: bool,
        server: Option<String>,
    },
    Blocks {
        users: Vec<UserRef>,
    },
    ChatMessage {
        channel: String,
        id: u64,
        time: i64,
        author: Author,
        content: String,
    },
    Groups {
        groups: Vec<GroupView>,
    },
    Reports {
        reports: Vec<ReportView>,
    },
    ReportCreated {
        report: ReportView,
    },
    Party {
        party: Option<PartyView>,
    },
    PartyInvite {
        party: Uuid,
        from: UserRef,
        expires: i64,
    },
    PartyWarp {
        from: UserRef,
        server: String,
    },
    PartyMemberState {
        member: Uuid,
        #[serde(skip_serializing_if = "Option::is_none")]
        position: Option<Position>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        inventory: Option<serde_json::Value>,
    },
}

#[derive(Serialize, Clone)]
pub struct PartyView {
    pub id: Uuid,
    pub leader: Uuid,
    pub locked: bool,
    pub pvp: bool,
    pub members: Vec<PartyMemberView>,
}

#[derive(Serialize, Clone)]
pub struct PartyMemberView {
    pub user: UserRef,
    pub role: PartyRole,
    pub online: bool,
    pub muted: bool,
    pub relation: Relation,
    pub player: Option<Player>,
    pub server: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub dimension: Option<String>,
}

impl Position {
    pub fn is_valid(&self) -> bool {
        [self.x, self.y, self.z].iter().all(|v| v.abs() <= MAX_COORDINATE)
            && self.yaw.is_finite()
            && self.pitch.is_finite()
            && self.dimension.as_ref().is_none_or(|dimension| dimension.len() <= 64)
    }
}

#[derive(Serialize, Clone)]
pub struct ReportView {
    pub id: Uuid,
    pub reporter: UserRef,
    pub target: UserRef,
    pub channel: Option<String>,
    pub message: Option<u64>,
    pub content: Option<String>,
    pub reason: String,
    pub time: i64,
}

#[derive(Serialize, Clone)]
pub struct GroupView {
    pub id: Uuid,
    pub name: String,
    pub role: Role,
    pub members: Vec<GroupMemberView>,
}

#[derive(Serialize, Clone)]
pub struct GroupMemberView {
    pub user: UserRef,
    pub role: Role,
    pub online: bool,
}

#[derive(Serialize, Clone)]
pub struct Author {
    #[serde(flatten)]
    pub user: UserRef,
    pub roles: Vec<RoleView>,
    pub highlight: bool,
}

#[derive(Serialize, Clone)]
pub struct RoleView {
    pub id: String,
    pub name: String,
    pub staff: bool,
}

#[derive(Serialize, Clone)]
pub struct SettingsView {
    pub allow_messages: bool,
    pub hide_server: bool,
    pub accept_friend_requests: bool,
    pub server_chat: bool,
}

#[derive(Serialize, Clone)]
pub struct FriendView {
    pub user: UserRef,
    pub since: i64,
    pub online: bool,
    pub server: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct UserRef {
    pub id: Uuid,
    pub kind: Kind,
    pub name: String,
    pub uuid: Uuid,
    /// The Minecraft account an online LiquidBounce Account proved it plays on.
    pub minecraft: Option<Player>,
}

impl From<&Identity> for UserRef {
    fn from(identity: &Identity) -> UserRef {
        UserRef {
            id: identity.id,
            kind: identity.kind,
            name: identity.name.clone(),
            uuid: identity.uuid,
            minecraft: None,
        }
    }
}

#[derive(Serialize, Clone)]
pub struct PunishmentView {
    pub id: Uuid,
    pub kind: punishment::Kind,
    pub ip: Option<String>,
    pub reason: String,
    pub issued_by: Option<UserRef>,
    pub created: i64,
    pub expires: Option<i64>,
}

impl ClientPacket {
    pub fn error(error: ClientError) -> ClientPacket {
        ClientPacket::Error {
            detail: error.detail(),
            message: error,
        }
    }

    pub fn error_with(error: ClientError, detail: impl Into<String>) -> ClientPacket {
        ClientPacket::Error {
            message: error,
            detail: Some(detail.into()),
        }
    }

    pub fn encode(&self, protocol: Protocol) -> Frame {
        let json = match self {
            ClientPacket::Error { message, detail: Some(_) } if protocol == Protocol::V1 => {
                serde_json::to_string(&ClientPacket::Error {
                    message: message.clone(),
                    detail: None,
                })
            }
            packet => serde_json::to_string(packet),
        };
        Frame(json.expect("could not encode packet").into())
    }
}

/// A serverbound packet
#[derive(Deserialize)]
#[serde(tag = "m", content = "c")]
pub enum ServerPacket {
    RequestMojangInfo,
    LoginMojang(User),
    LoginJWT(IgnoredAny),
    LoginAccount { token: String, allow_messages: bool },
    RequestJWT,
    Message { content: String },
    PrivateMessage { receiver: String, content: String },
    BanUser { user: Uuid },
    UnbanUser { user: Uuid },
    RequestUserCount,
    Hello { protocol: u32 },
    Punish {
        user: Option<String>,
        ip: Option<String>,
        kind: punishment::Kind,
        /// Seconds; `None` is permanent.
        duration: Option<u64>,
        reason: String,
        #[serde(default)]
        include_ip: bool,
    },
    Pardon { user: Option<String>, ip: Option<String> },
    RequestPunishments { user: String },
    Settings {
        allow_messages: Option<bool>,
        hide_server: Option<bool>,
        accept_friend_requests: Option<bool>,
        server_chat: Option<bool>,
    },
    Friend { action: FriendAction, user: String },
    Block { user: String, blocked: bool },
    ChatMessage { channel: String, content: String },
    Group(GroupAction),
    Report { user: String, message: Option<u64>, reason: String },
    RequestReports,
    ResolveReport { id: Uuid },
    Party(PartyAction),
    Location {
        server: Option<String>,
        world: Option<World>,
        player: Option<Player>,
    },
    Sightings {
        #[serde(default)]
        entities: Vec<Uuid>,
        #[serde(default)]
        tab: Vec<Uuid>,
    },
    PartyState {
        position: Option<Position>,
        status: Option<serde_json::Value>,
        inventory: Option<serde_json::Value>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum PartyAction {
    Invite { user: String },
    Accept { party: Uuid },
    Decline { party: Uuid },
    Leave,
    Kick { user: String },
    Promote { user: String, admin: bool },
    Transfer { user: String },
    Lock { locked: bool },
    Mute { user: String, muted: bool },
    Pvp { enabled: bool },
    Warp,
    Disband,
}

#[derive(Debug, Clone, Deserialize)]
pub struct World {
    pub dimension: String,
    #[serde(deserialize_with = "seed")]
    pub seed: i64,
    pub age: Option<i64>,
}

/// JavaScript cannot hold the 64-bit hashed seed as a number, so it may come as a string.
fn seed<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Seed {
        Number(i64),
        Text(String),
    }
    match Seed::deserialize(deserializer)? {
        Seed::Number(seed) => Ok(seed),
        Seed::Text(seed) => seed.parse().map_err(serde::de::Error::custom),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum GroupAction {
    Create { name: String },
    Rename { group: Uuid, name: String },
    Invite { group: Uuid, user: String },
    Accept { group: Uuid },
    Decline { group: Uuid },
    Leave { group: Uuid },
    Kick { group: Uuid, user: String },
    Promote { group: Uuid, user: String, admin: bool },
    Delete { group: Uuid },
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FriendAction {
    Request,
    Accept,
    Decline,
    Remove,
}

/// The author of a v1 message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub name: String,
    pub uuid: Uuid,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct User {
    pub name: String,
    pub uuid: Uuid,
    /// Should this user allow private messages?
    pub allow_messages: bool,
}

#[derive(Serialize, Deserialize, Copy, Clone)]
pub enum SuccessReason {
    Login,
    Ban,
    Unban,
    Punish,
    Pardon,
    Report,
    Resolve,
    /// A LiquidBounce Account session proved the Minecraft account it plays on.
    Minecraft,
}

#[cfg(test)]
mod tests {
    use super::{ClientError, ClientPacket, Protocol, ServerPacket, SuccessReason, User, UserInfo};
    use serde_json::json;

    fn notch() -> UserInfo {
        UserInfo {
            name: "Notch".into(),
            uuid: "069a79f4-44e9-4726-a5be-fca90e38aaf5".parse().unwrap(),
        }
    }

    fn encode(packet: ClientPacket, protocol: Protocol) -> serde_json::Value {
        serde_json::from_str(&packet.encode(protocol).0).unwrap()
    }

    #[test]
    fn client_packets() {
        let author = json!({ "name": "Notch", "uuid": "069a79f4-44e9-4726-a5be-fca90e38aaf5" });
        let cases = [
            (
                ClientPacket::MojangInfo { session_hash: "88e16a1019277b15d58faf0541e11910eb756f6".into() },
                json!({ "m": "MojangInfo", "c": { "session_hash": "88e16a1019277b15d58faf0541e11910eb756f6" } }),
            ),
            (
                ClientPacket::Message { author_info: notch(), content: "Hello, World!".into() },
                json!({ "m": "Message", "c": { "author_info": author, "content": "Hello, World!" } }),
            ),
            (
                ClientPacket::PrivateMessage { author_info: notch(), content: "Hello, User!".into() },
                json!({ "m": "PrivateMessage", "c": { "author_info": author, "content": "Hello, User!" } }),
            ),
            (
                ClientPacket::UserCount { connections: 623, logged_in: 531 },
                json!({ "m": "UserCount", "c": { "connections": 623, "logged_in": 531 } }),
            ),
            (
                ClientPacket::Success { reason: SuccessReason::Login },
                json!({ "m": "Success", "c": { "reason": "Login" } }),
            ),
            (
                ClientPacket::Success { reason: SuccessReason::Ban },
                json!({ "m": "Success", "c": { "reason": "Ban" } }),
            ),
            (
                ClientPacket::Success { reason: SuccessReason::Unban },
                json!({ "m": "Success", "c": { "reason": "Unban" } }),
            ),
            (
                ClientPacket::error(ClientError::LoginFailed),
                json!({ "m": "Error", "c": { "message": "LoginFailed" } }),
            ),
            // the object form `{"InvalidCharacter":"§"}` made old clients drop the connection
            (
                ClientPacket::error(ClientError::InvalidCharacter('§')),
                json!({ "m": "Error", "c": { "message": "InvalidCharacter" } }),
            ),
        ];
        for (packet, expected) in cases {
            assert_eq!(encode(packet, Protocol::V1), expected);
        }
    }

    #[test]
    fn v2_errors_carry_details() {
        assert_eq!(
            encode(ClientPacket::error(ClientError::InvalidCharacter('§')), Protocol::V2),
            json!({ "m": "Error", "c": { "message": "InvalidCharacter", "detail": "§" } })
        );
        assert_eq!(
            encode(ClientPacket::error(ClientError::LoginFailed), Protocol::V2),
            json!({ "m": "Error", "c": { "message": "LoginFailed" } })
        );
        assert_eq!(
            encode(ClientPacket::Hello { protocol: 2 }, Protocol::V2),
            json!({ "m": "Hello", "c": { "protocol": 2 } })
        );
    }

    #[test]
    fn server_packets() {
        let decode = |value: serde_json::Value| -> ServerPacket { serde_json::from_value(value).unwrap() };

        assert!(matches!(decode(json!({ "m": "RequestMojangInfo" })), ServerPacket::RequestMojangInfo));
        assert!(matches!(decode(json!({ "m": "RequestJWT" })), ServerPacket::RequestJWT));
        assert!(matches!(decode(json!({ "m": "RequestUserCount" })), ServerPacket::RequestUserCount));
        assert!(matches!(
            decode(json!({ "m": "LoginMojang", "c": { "name": "Notch", "uuid": "069a79f444e94726a5befca90e38aaf5", "allow_messages": true } })),
            ServerPacket::LoginMojang(User { ref name, allow_messages: true, .. }) if name == "Notch"
        ));
        assert!(matches!(
            decode(json!({ "m": "LoginJWT", "c": { "token": "token", "allow_messages": false } })),
            ServerPacket::LoginJWT(_)
        ));
        assert!(matches!(
            decode(json!({ "m": "Message", "c": { "content": "Hello, World!" } })),
            ServerPacket::Message { ref content } if content == "Hello, World!"
        ));
        assert!(matches!(
            decode(json!({ "m": "PrivateMessage", "c": { "content": "Hello, Notch!", "receiver": "Notch" } })),
            ServerPacket::PrivateMessage { ref receiver, .. } if receiver == "Notch"
        ));
        assert!(matches!(
            decode(json!({ "m": "BanUser", "c": { "user": "069a79f4-44e9-4726-a5be-fca90e38aaf5" } })),
            ServerPacket::BanUser { .. }
        ));
        assert!(matches!(
            decode(json!({ "m": "UnbanUser", "c": { "user": "069a79f4-44e9-4726-a5be-fca90e38aaf5" } })),
            ServerPacket::UnbanUser { .. }
        ));
        assert!(matches!(decode(json!({ "m": "Hello", "c": { "protocol": 2 } })), ServerPacket::Hello { protocol: 2 }));
    }
}
