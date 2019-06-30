use crate::error::{ClientError, Error};

use actix::{
    dev::{MessageResponse, ResponseChannel},
    *,
};
use ring::digest::{digest, SHA256};
use serde::{
    de::{self, Deserializer, Visitor},
    ser::Serializer,
    Deserialize, Serialize,
};
use std::{
    convert::TryInto,
    fmt::{self, Write},
    str::FromStr,
};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Copy, Clone, Eq, PartialEq, Hash)]
#[serde(transparent)]
pub struct InternalId(u64);

impl InternalId {
    pub fn new(id: u64) -> InternalId {
        InternalId(id)
    }
}

impl fmt::Display for InternalId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}

impl<A, M> MessageResponse<A, M> for InternalId
where
    A: Actor,
    M: Message<Result = InternalId>,
{
    fn handle<R: ResponseChannel<M>>(self, _: &mut A::Context, tx: Option<R>) {
        if let Some(tx) = tx {
            tx.send(self);
        }
    }
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Id([u8; 32]);

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_char('#')?;

        for i in &self.0 {
            write!(f, "{:02x}", i)?;
        }

        Ok(())
    }
}

impl From<Uuid> for Id {
    fn from(uuid: Uuid) -> Id {
        Id(digest(&SHA256, uuid.as_bytes())
            .as_ref()
            .try_into()
            .unwrap())
    }
}

impl FromStr for Id {
    type Err = Error;

    fn from_str(s: &str) -> Result<Id, Error> {
        let mut bytes = [0; 32];

        for (i, c) in s.chars().enumerate() {
            let m = c
                .to_digit(16)
                .ok_or_else(|| Error::AxoChat(ClientError::InvalidId))? as u8;

            if i % 2 == 0 {
                bytes[i / 2] = m << 4;
            } else {
                bytes[i / 2] |= m;
            }
        }

        Ok(Id(bytes))
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D>(deserializer: D) -> Result<Id, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct IdVisitor;

        impl<'de> Visitor<'de> for IdVisitor {
            type Value = Id;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a sha256 hex string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Id::from_str(value).or_else(|err| Err(E::custom(err)))
            }
        }

        deserializer.deserialize_str(IdVisitor)
    }
}

impl Serialize for Id {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut s = String::with_capacity(64);
        for i in &self.0 {
            write!(&mut s, "{:02x}", i).unwrap();
        }
        serializer.serialize_str(&format!("{}", self))
    }
}
