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

    /// `perks` additionally allows `§` formatting codes and emoji.
    pub fn validate(&self, msg: &str, perks: bool) -> Result<()> {
        if msg.is_empty() {
            return Err(ClientError::EmptyMessage.into());
        }

        let mut previous: Option<char> = None;
        let mut combining = 0;
        let mut chars = msg.chars().enumerate().peekable();
        while let Some((char_index, ch)) = chars.next() {
            if char_index >= self.cfg.max_length {
                return Err(ClientError::MessageTooLong.into());
            }
            let allowed = if ch == ' ' || ch.is_ascii_graphic() || ch.is_alphanumeric() {
                true
            } else if !perks {
                false
            } else if ch == '§' {
                // §k obfuscates text and is left out
                let code = chars.next_if(|(_, code)| "0123456789abcdeflmnorABCDEFLMNOR".contains(*code));
                previous = code.map(|(_, code)| code);
                if code.is_none() {
                    return Err(ClientError::InvalidCharacter(ch).into());
                }
                continue;
            } else if is_combining(ch) {
                combining += 1;
                combining <= 2
            } else if ch == '\u{200d}' || ch == '\u{fe0f}' {
                // joiners and variation selectors only make sense inside emoji sequences
                previous.is_some_and(|previous| !previous.is_ascii() && !previous.is_alphanumeric())
            } else {
                !ch.is_control() && !is_invisible(ch)
            };
            if !allowed {
                return Err(ClientError::InvalidCharacter(ch).into());
            }
            if !is_combining(ch) {
                combining = 0;
            }
            previous = Some(ch);
        }

        Ok(())
    }
}

fn is_combining(ch: char) -> bool {
    matches!(ch, '\u{300}'..='\u{36f}' | '\u{1ab0}'..='\u{1aff}' | '\u{1dc0}'..='\u{1dff}' | '\u{20d0}'..='\u{20ff}' | '\u{fe20}'..='\u{fe2f}')
}

/// Text direction overrides and zero-width characters, which disguise what a message says.
fn is_invisible(ch: char) -> bool {
    matches!(ch, '\u{200b}' | '\u{200c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
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

#[cfg(test)]
mod validator_tests {
    use super::MessageValidator;
    use crate::config::MsgConfig;
    use crate::error::{ClientError, Error};
    use std::time::Duration;

    fn check(msg: &str, perks: bool) -> Result<(), ClientError> {
        let validator = MessageValidator::new(MsgConfig {
            max_length: 20,
            max_messages: 10,
            count_duration: Duration::from_secs(60),
            perk_roles: Vec::new(),
        });
        validator.validate(msg, perks).map_err(|err| match err {
            Error::AxoChat { source } => source,
            err => panic!("{}", err),
        })
    }

    #[test]
    fn everyone() {
        assert_eq!(check("Hello, Wörld! 你好", false), Ok(()));
        assert_eq!(check("", false), Err(ClientError::EmptyMessage));
        assert_eq!(check("x".repeat(21).as_str(), false), Err(ClientError::MessageTooLong));
        assert_eq!(check("§cred", false), Err(ClientError::InvalidCharacter('§')));
        assert_eq!(check("😀", false), Err(ClientError::InvalidCharacter('😀')));
    }

    #[test]
    fn donators() {
        assert_eq!(check("§cred §lbold§r", true), Ok(()));
        assert_eq!(check("§kmagic", true), Err(ClientError::InvalidCharacter('§')));
        assert_eq!(check("dangling§", true), Err(ClientError::InvalidCharacter('§')));
        assert_eq!(check("hi 😀👍", true), Ok(()));
        assert_eq!(check("👨\u{200d}👩\u{200d}👧", true), Ok(()), "family emoji");
        assert_eq!(check("❤\u{fe0f}", true), Ok(()));
        assert_eq!(check("a\u{200d}b", true), Err(ClientError::InvalidCharacter('\u{200d}')));
        assert_eq!(check("evil\u{202e}txt", true), Err(ClientError::InvalidCharacter('\u{202e}')));
        assert_eq!(check("zero\u{200b}width", true), Err(ClientError::InvalidCharacter('\u{200b}')));
        assert_eq!(check("tab\there", true), Err(ClientError::InvalidCharacter('\t')));
        assert_eq!(check("e\u{301}\u{302}", true), Ok(()));
        assert_eq!(check("e\u{301}\u{302}\u{303}", true), Err(ClientError::InvalidCharacter('\u{303}')), "zalgo");
    }
}
