use crate::config::ModConfig;
use crate::error::*;
use std::collections::HashSet;
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
};
use uuid::Uuid;

pub struct Moderation {
    config: ModConfig,
    banned: HashSet<Uuid>,
}

impl Moderation {
    pub fn new(config: ModConfig) -> Result<Moderation> {
        let banned = read_ids(&config.banned)?;
        Ok(Moderation { config, banned })
    }

    pub fn ban(&mut self, user: &Uuid) -> Result<()> {
        if self.banned.insert(*user) {
            let mut file = OpenOptions::new()
                .append(true)
                .create(true)
                .open(&self.config.banned)?;

            writeln!(file, "{}", user.hyphenated())?;
        }

        Ok(())
    }

    pub fn unban(&mut self, user: &Uuid) -> Result<()> {
        if self.banned.remove(user) {
            let mut writer = BufWriter::new(File::create(&self.config.banned)?);

            for banned in &self.banned {
                writeln!(writer, "{}", banned)?;
            }

            Ok(())
        } else {
            Err(ClientError::NotBanned.into())
        }
    }

    pub fn is_banned(&self, user: &Uuid) -> bool {
        self.banned.contains(user)
    }
}

fn read_ids(path: &Path) -> Result<HashSet<Uuid>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(ref err) if err.kind() == std::io::ErrorKind::NotFound => {
            File::create(path)?;
            return Ok(HashSet::new());
        }
        Err(err) => return Err(err.into()),
    };
    let reader = BufReader::new(file);
    let mut lines = HashSet::new();
    for line in reader.lines() {
        let line = line?;
        if !line.is_empty() {
            lines.insert(line.parse()?);
        }
    }
    Ok(lines)
}
