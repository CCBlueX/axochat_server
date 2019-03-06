use std::{error, fmt, io};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    IO(io::Error),
    CBOR(serde_cbor::error::Error),
    TOML(toml::de::Error),
    Actix(actix_web::Error),
    OpenSSL(openssl::error::ErrorStack),
    LoginFailed,
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::IO(err) => Some(err),
            Error::CBOR(err) => Some(err),
            Error::TOML(err) => Some(err),
            Error::OpenSSL(err) => Some(err),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::IO(err) => write!(f, "I/O: {}", err),
            Error::CBOR(err) => write!(f, "JSON: {}", err),
            Error::TOML(err) => write!(f, "TOML: {}", err),
            Error::Actix(err) => write!(f, "actix-web: {}", err),
            Error::OpenSSL(err) => write!(f, "OpenSSL: {}", err),
            Error::LoginFailed => write!(f, "login failed"),
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::IO(err)
    }
}

impl From<serde_cbor::error::Error> for Error {
    fn from(err: serde_cbor::error::Error) -> Error {
        Error::CBOR(err)
    }
}

impl From<toml::de::Error> for Error {
    fn from(err: toml::de::Error) -> Error {
        Error::TOML(err)
    }
}

impl From<actix_web::Error> for Error {
    fn from(err: actix_web::Error) -> Error {
        Error::Actix(err)
    }
}

impl From<openssl::error::ErrorStack> for Error {
    fn from(err: openssl::error::ErrorStack) -> Error {
        Error::OpenSSL(err)
    }
}
