use derive_more::From;
use serde::Serialize;
use snafu::Snafu;
use std::{error, fmt, io};

pub type Result<T> = std::result::Result<T, Error>;

// Removed the From derive to avoid conflicts with manually implemented From traits
#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("I/O: {}", source))]
    IO { source: io::Error },
    #[snafu(display("JSON: {}", source))]
    JSON { source: serde_json::error::Error },
    #[snafu(display("TOML: {}", source))]
    TOML { source: toml::de::Error },
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
    #[snafu(display("JWT: {}", source))]
    JWT { source: jsonwebtoken::errors::Error },
    #[snafu(display("UUID parsing: {}", source))]
    Uuid { source: uuid::Error },
    #[snafu(display("axochat: {}", source))]
    AxoChat { source: ClientError },
}

// Manually implement From traits to avoid conflicts
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

impl From<toml::de::Error> for Error {
    fn from(source: toml::de::Error) -> Self {
        Error::TOML { source }
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

impl From<jsonwebtoken::errors::Error> for Error {
    fn from(source: jsonwebtoken::errors::Error) -> Self {
        Error::JWT { source }
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
        use self::ClientError::*;

        match self {
            NotSupported => write!(f, "method not supported"),
            LoginFailed => write!(f, "login failed"),
            NotLoggedIn => write!(f, "not logged in"),
            AlreadyLoggedIn => write!(f, "already logged in"),
            MojangRequestMissing => write!(f, "mojang request missing"),
            NotPermitted => write!(f, "not permitted"),
            NotBanned => write!(f, "not banned"),
            Banned => write!(f, "banned"),
            RateLimited => write!(f, "rate limited"),
            PrivateMessageNotAccepted => write!(f, "private message not accepted"),
            EmptyMessage => write!(f, "empty message"),
            MessageTooLong => write!(f, "message was too long"),
            InvalidCharacter(ch) => write!(
                f,
                "message contained invalid character: `{}`",
                ch.escape_default()
            ),
            InvalidId => write!(f, "invalid id"),
            Internal => write!(f, "internal error"),
        }
    }
}
