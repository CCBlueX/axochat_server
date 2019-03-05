mod auth;
mod chat;
mod config;
mod error;

use error::*;
use log::*;

use actix::{Arbiter, System};
use actix_web::{server::HttpServer, App};

fn main() -> Result<()> {
    env_logger::init();
    let config = config::read_config()?;
    debug!("Read configuration file: {:?}", config);

    let system = System::new("axochat");
    let server = Arbiter::start(|_| chat::ChatServer::default());

    HttpServer::new(move || {
        let server_state = chat::ServerState {
            addr: server.clone(),
        };
        App::with_state(server_state).resource("/ws", |r| r.route().f(chat::chat_route))
    })
    .bind(config.net.address)?
    .start();

    info!("Started server at {}", config.net.address);
    system.run();

    Ok(())
}
