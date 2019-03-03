mod chat;
mod config;
mod error;

use error::*;
use log::*;

use actix_web::{actix::System, server::HttpServer, App};

fn main() -> Result<()> {
    env_logger::init();
    let config = config::read_config()?;
    debug!("Read configuration file: {:?}", config);

    let system = System::new("axochat");

    HttpServer::new(move || {
        let server_state = chat::ServerState {};
        App::with_state(server_state).resource("/ws", |r| r.route().f(chat::chat_route))
    })
    .bind(config.net.address)?
    .start();

    info!("Started server at {}", config.net.address);
    system.run();

    Ok(())
}
