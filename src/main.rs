mod error;
mod config;

use log::*;
use error::*;

fn main() -> Result<()> {
    env_logger::init();
    let config = config::read_config()?;
    debug!("Read configuration file: {:?}", config);

    Ok(())
}
