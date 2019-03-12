use crate::error::*;
use crate::config::ModConfig;
use hashbrown::HashSet;
use std::{fs::{File, OpenOptions}, io::{BufReader, BufRead, BufWriter, Write}, path::Path};

pub struct Moderation {
    config: ModConfig,
    moderators: HashSet<String>,
    banned: HashSet<String>,
}

impl Moderation {
    pub fn new(config: ModConfig) -> Result<Moderation> {
        let moderators = read_lines(&config.moderators)?;
        let banned = read_lines(&config.banned)?;
        Ok(Moderation { config, moderators, banned })
    }

    pub fn is_moderator(&self, user: &str) -> bool {
        self.moderators.contains(user)
    }

    /// Ban user if user is not a moderator.
    pub fn ban(&mut self, user: &str) -> Result<()> {
        if self.is_moderator(user) {
            Err(Error::AxoChat(ClientError::NotPermitted))
        } else {
            if self.banned.insert(user.to_owned()) {
                let mut file = OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&self.config.banned)?;

                writeln!(file, "{}", user)?;
            }

            Ok(())
        }
    }

    pub fn unban(&mut self, user: &str) -> Result<()> {
        if self.banned.remove(user) {
            write_lines(&self.config.banned, self.banned.iter())?;
        }

        Ok(())
    }
}

fn read_lines(path: &Path) -> Result<HashSet<String>> {
    let reader = BufReader::new(File::open(&path)?);
    let mut lines = HashSet::new();
    for line in reader.lines() {
        let line = line?;
        if !line.is_empty() {
            lines.insert(line);
        }
    }
    Ok(lines)
}

fn write_lines<'a>(path: &Path, lines: impl Iterator<Item = &'a String>) -> Result<()> {
    let mut writer = BufWriter::new(File::create(&path)?);

    for line in lines {
        writeln!(writer, "{}", line)?;
    }

    Ok(())
}
