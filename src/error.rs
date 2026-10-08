use serde::{Serialize, Serializer};
use snafu::Snafu;
use std::{error, fmt, io};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("I/O: {}", source))]
    IO { source: io::Error },
    #[snafu(display("JSON: {}", source))]
    JSON { source: serde_json::error::Error },
    #[snafu(display("actix-web: {}", source))]
    Actix { source: actix_web::Error },
    #[cfg(feature = "openssl-tls")]
    #[snafu(display("OpenSSL: {}", source))]
    OpenSSL { source: openssl::error::ErrorStack },
    #[cfg(feature = "rustls-tls")]
    #[snafu(display("rustls: {}", source))]
    RustTLS { source: std::io::Error },
    #[cfg(feature = "rustls-tls")]
    #[snafu(display("rustls"))]
    RustTLSNoMsg,
    #[snafu(display("database: {}", source))]
    Database { source: sea_orm::DbErr },
    #[snafu(display("UUID parsing: {}", source))]
    Uuid { source: uuid::Error },
    #[snafu(display("axochat: {}", source))]
    AxoChat { source: ClientError },
}

impl From<io::Error> for Error {
    fn from(source: io::Error) -> Self {
        Error::IO { source }
    }
}

impl From<serde_json::error::Error> for Error {
    fn from(source: serde_json::error::Error) -> Self {
        Error::JSON { source }
    }
}

impl From<actix_web::Error> for Error {
    fn from(source: actix_web::Error) -> Self {
        Error::Actix { source }
    }
}

#[cfg(feature = "openssl-tls")]
impl From<openssl::error::ErrorStack> for Error {
    fn from(source: openssl::error::ErrorStack) -> Self {
        Error::OpenSSL { source }
    }
}

impl From<uuid::Error> for Error {
    fn from(source: uuid::Error) -> Self {
        Error::Uuid { source }
    }
}

impl From<ClientError> for Error {
    fn from(source: ClientError) -> Self {
        Error::AxoChat { source }
    }
}

/// A client-facing error, sent as its name.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientError {
    NotSupported,
    LoginFailed,
    NotLoggedIn,
    AlreadyLoggedIn,
    MojangRequestMissing,
    NotPermitted,
    NotBanned,
    Banned,
    RateLimited,
    PrivateMessageNotAccepted,
    EmptyMessage,
    MessageTooLong,
    InvalidCharacter(char),
    InvalidId,
    Internal,
    InvalidPacket,
    Muted,
    UnknownUser,
    AlreadyFriends,
    NotFriends,
    NoInvite,
    RequestsDisabled,
    UnknownChannel,
}

impl ClientError {
    pub fn code(&self) -> &'static str {
        use self::ClientError::*;

        match self {
            NotSupported => "NotSupported",
            LoginFailed => "LoginFailed",
            NotLoggedIn => "NotLoggedIn",
            AlreadyLoggedIn => "AlreadyLoggedIn",
            MojangRequestMissing => "MojangRequestMissing",
            NotPermitted => "NotPermitted",
            NotBanned => "NotBanned",
            Banned => "Banned",
            RateLimited => "RateLimited",
            PrivateMessageNotAccepted => "PrivateMessageNotAccepted",
            EmptyMessage => "EmptyMessage",
            MessageTooLong => "MessageTooLong",
            InvalidCharacter(_) => "InvalidCharacter",
            InvalidId => "InvalidId",
            Internal => "Internal",
            InvalidPacket => "InvalidPacket",
            Muted => "Muted",
            UnknownUser => "UnknownUser",
            AlreadyFriends => "AlreadyFriends",
            NotFriends => "NotFriends",
            NoInvite => "NoInvite",
            RequestsDisabled => "RequestsDisabled",
            UnknownChannel => "UnknownChannel",
        }
    }

    pub fn detail(&self) -> Option<String> {
        match self {
            ClientError::InvalidCharacter(ch) => Some(ch.to_string()),
            _ => None,
        }
    }
}

impl Serialize for ClientError {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

impl error::Error for ClientError {}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ClientError::InvalidCharacter(ch) => write!(
                f,
                "message contained invalid character: `{}`",
                ch.escape_default()
            ),
            error => f.write_str(error.code()),
        }
    }
}

impl From<sea_orm::DbErr> for Error {
    fn from(source: sea_orm::DbErr) -> Self {
        Error::Database { source }
    }
}
