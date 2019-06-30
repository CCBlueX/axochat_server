use crate::error::*;

use crate::config::MsgConfig;
use std::{collections::VecDeque, time::Instant};

pub struct RateLimiter {
    buf: VecDeque<Instant>,
    cfg: MsgConfig,
}

impl RateLimiter {
    pub fn new(cfg: MsgConfig) -> RateLimiter {
        RateLimiter {
            buf: VecDeque::with_capacity(cfg.max_messages),
            cfg,
        }
    }

    /// Returns if a new message in this instant would be rate limited.
    /// If not, then it registers the new message instant.
    pub fn check_new_message(&mut self) -> bool {
        let now = Instant::now();
        let limit = now - *self.cfg.count_duration;

        #[allow(clippy::op_ref)]
        let last_index = self
            .buf
            .iter()
            .take_while(|time| *time < &limit)
            .enumerate()
            .map(|(i, _)| i)
            .last()
            .unwrap_or(0);
        self.buf.drain(..last_index);

        if self.buf.len() < self.cfg.max_messages {
            self.buf.push_back(now);
            false
        } else {
            true
        }
    }
}

pub struct MessageValidator {
    cfg: MsgConfig,
}

impl MessageValidator {
    pub fn new(cfg: MsgConfig) -> MessageValidator {
        MessageValidator { cfg }
    }

    pub fn validate(&self, msg: &str) -> Result<()> {
        if msg.is_empty() {
            return Err(ClientError::EmptyMessage.into());
        }

        for (char_index, ch) in msg.chars().enumerate() {
            if char_index >= self.cfg.max_length {
                return Err(ClientError::MessageTooLong.into());
            }
            if ch != ' ' && !ch.is_ascii_graphic() && !ch.is_alphanumeric() {
                return Err(ClientError::InvalidCharacter(ch).into());
            }
        }

        Ok(())
    }
}
