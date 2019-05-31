use serde::Serialize;
use std::{error, fmt, io};
use derive_more::From;
use failure::Fail;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, From, Fail)]
pub enum Error {
    #[fail(display = "I/O: {}", _0)]
    IO(io::Error),
    #[fail(display = "JSON: {}", _0)]
    JSON(serde_json::error::Error),
    #[fail(display = "TOML: {}", _0)]
    TOML(toml::de::Error),
    #[fail(display = "actix-web: {}", _0)]
    Actix(actix_web::Error),
    #[cfg(feature = "ssl")]
    #[fail(display = "OpenSSL: {}", _0)]
    OpenSSL(openssl::error::ErrorStack),
    #[cfg(feature = "rust-tls")]
    #[fail(display = "rustls: {}", _0)]
    RustTLS(rustls::TLSError),
    #[cfg(feature = "rust-tls")]
    #[fail(display = "rustls")]
    RustTLSNoMsg,
    #[fail(display = "JWT: {}", _0)]
    JWT(jsonwebtoken::errors::Error),
    #[fail(display = "axochat: {}", _0)]
    AxoChat(ClientError),
}

/// A client-facing error.
#[derive(Debug, Clone, Serialize)]
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
}

impl error::Error for ClientError {}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ClientError::NotSupported => write!(f, "method not supported"),
            ClientError::LoginFailed => write!(f, "login failed"),
            ClientError::NotLoggedIn => write!(f, "not logged in"),
            ClientError::AlreadyLoggedIn => write!(f, "already logged in"),
            ClientError::MojangRequestMissing => write!(f, "mojang request missing"),
            ClientError::NotPermitted => write!(f, "not permitted"),
            ClientError::NotBanned => write!(f, "not banned"),
            ClientError::Banned => write!(f, "banned"),
            ClientError::RateLimited => write!(f, "rate limited"),
            ClientError::PrivateMessageNotAccepted => write!(f, "private message not accepted"),
            ClientError::EmptyMessage => write!(f, "empty message"),
            ClientError::MessageTooLong => write!(f, "message was too long"),
            ClientError::InvalidCharacter(ch) => write!(
                f,
                "message contained invalid character: `{}`",
                ch.escape_default()
            ),
            ClientError::InvalidId => write!(f, "invalid id"),
            ClientError::Internal => write!(f, "internal error"),
        }
    }
}
