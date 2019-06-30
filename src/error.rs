use derive_more::From;
use serde::Serialize;
use snafu::Snafu;
use std::{error, fmt, io};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Snafu, From)]
pub enum Error {
    #[snafu(display("I/O: {}", source))]
    IO { source: io::Error },
    #[snafu(display("JSON: {}", source))]
    JSON { source: serde_json::error::Error },
    #[snafu(display("TOML: {}", source))]
    TOML { source: toml::de::Error },
    #[snafu(display("actix-web: {}", source))]
    Actix { source: actix_web::Error },
    #[cfg(feature = "ssl")]
    #[snafu(display("OpenSSL: {}", source))]
    OpenSSL { source: openssl::error::ErrorStack },
    #[cfg(feature = "rust-tls")]
    #[snafu(display("rustls: {}", source))]
    RustTLS { source: rustls::TLSError },
    #[cfg(feature = "rust-tls")]
    #[snafu(display("rustls"))]
    RustTLSNoMsg,
    #[snafu(display("JWT: {}", source))]
    JWT { source: jsonwebtoken::errors::Error },
    #[snafu(display("axochat: {}", source))]
    AxoChat { source: ClientError },
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
