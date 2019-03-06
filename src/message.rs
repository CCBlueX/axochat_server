use crate::config::MsgConfig;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub struct Ratelimiter {
    buf: VecDeque<Instant>,
    cfg: MsgConfig,
}

impl Ratelimiter {
    pub fn new(cfg: MsgConfig) -> Ratelimiter {
        Ratelimiter {
            buf: VecDeque::with_capacity(cfg.max_messages),
            cfg,
        }
    }

    /// Returns if a new message in this instant would be rate limited.
    /// If not, then it registers the new message instant.
    pub fn check_new_message(&mut self) -> bool {
        let now = Instant::now();
        let limit = now - self.cfg.count_duration;
        self.buf
            .drain(..)
            .take_while(|time| time < &limit)
            .for_each(|_| {});

        false
    }
}
