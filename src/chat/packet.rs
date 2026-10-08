use super::Frame;
use crate::auth::UserInfo;
use crate::error::ClientError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL: u32 = 2;

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
    NewJWT {
        token: String,
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
    LoginJWT { token: String, allow_messages: bool },
    LoginAccount { token: String, allow_messages: bool },
    RequestJWT,
    Message { content: String },
    PrivateMessage { receiver: String, content: String },
    BanUser { user: Uuid },
    UnbanUser { user: Uuid },
    RequestUserCount,
    Hello { protocol: u32 },
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
                ClientPacket::NewJWT { token: "token".into() },
                json!({ "m": "NewJWT", "c": { "token": "token" } }),
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
            ServerPacket::LoginJWT { ref token, allow_messages: false } if token == "token"
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
