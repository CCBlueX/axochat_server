use crate::error::*;

use crate::config::MsgConfig;
use std::{collections::VecDeque, time::Instant};

pub struct RateLimiter {
    buf: VecDeque<(Instant, String)>,
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
    pub fn check_new_message(&mut self, message: String) -> bool {
        let now = Instant::now();
        while self
            .buf
            .front()
            .is_some_and(|(time, _)| now.duration_since(*time) >= self.cfg.count_duration)
        {
            self.buf.pop_front();
        }

        if self.buf.len() < self.cfg.max_messages {
            let message_found = self.buf.iter().any(|(_, msg)| &message == msg);
            if message_found {
                true
            } else {
                self.buf.push_back((now, message));
                false
            }
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

/// A token bucket for packets that are not chat messages.
pub struct ActionLimiter {
    burst: f64,
    per_second: f64,
    tokens: f64,
    last: Instant,
}

impl ActionLimiter {
    pub fn new(burst: u32, per_second: f64) -> ActionLimiter {
        ActionLimiter {
            burst: burst as f64,
            per_second,
            tokens: burst as f64,
            last: Instant::now(),
        }
    }

    pub fn allow(&mut self) -> bool {
        let now = Instant::now();
        self.tokens = (self.tokens + now.duration_since(self.last).as_secs_f64() * self.per_second).min(self.burst);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ActionLimiter;

    #[test]
    fn action_limiter() {
        let mut limiter = ActionLimiter::new(3, 0.0);
        assert!(limiter.allow() && limiter.allow() && limiter.allow());
        assert!(!limiter.allow());
    }
}
