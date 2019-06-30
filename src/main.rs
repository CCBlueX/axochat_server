mod auth;
mod chat;
mod config;
mod error;
mod message;
mod moderation;

use config::Config;
use error::*;
use log::*;
use structopt::*;

use actix::*;
use actix_web::{web, App, HttpServer};
use uuid::Uuid;

#[cfg(feature = "rust-tls")]
use {
    rustls::{
        internal::pemfile::{certs, rsa_private_keys},
        NoClientAuth, ServerConfig,
    },
    std::{fs::File, io::BufReader},
};

#[cfg(feature = "ssl")]
use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};

#[derive(StructOpt)]
enum Opt {
    /// Starts the axochat server.
    #[structopt(name = "start")]
    Start,
    /// Generates a JWT which can be used for logging in.
    /// This should only be used for testing.
    /// If you want to generate JWT for non-testing purposes, send a RequestJWT packet to the server.
    #[structopt(name = "generate")]
    Generate {
        #[structopt(name = "name")]
        name: String,
        #[structopt(name = "uuid")]
        uuid: Option<Uuid>,
    },
}

fn main() -> Result<()> {
    env_logger::init();

    let config = config::read_config()?;
    debug!("Read configuration file: {:?}", config);

    let opt = Opt::from_args();
    match opt {
        Opt::Start => start_server(config),
        Opt::Generate { name, uuid } => {
            let auth = match config.auth {
                Some(auth) => auth::Authenticator::new(&auth),
                None => {
                    eprintln!("Please add a `auth` segment to your configuration file.");
                    Err(ClientError::NotSupported.into())
                }
            }?;
            let token = auth.new_token(auth::UserInfo {
                name,
                uuid: uuid.unwrap_or_else(|| Uuid::from_u128(0)),
            })?;
            println!("{}", token);
            Ok(())
        }
    }
}

fn start_server(config: Config) -> Result<()> {
    let system = System::new("axochat");
    let server_config = config.clone();
    let server = chat::ChatServer::new(server_config).start();

    let server = HttpServer::new(move || {
        let server_state = chat::ServerState {
            addr: server.clone(),
        };
        App::new()
            .data(server_state)
            .service(web::resource("/ws").to(chat::chat_route))
    });

    if let (Some(cert), Some(key)) = (config.net.cert_file, config.net.key_file) {
        #[cfg(all(feature = "ssl", feature = "rust-tls"))]
        {
            compile_error!("Can't enable both the `ssl` and the `rust-tls` feature.")
        }

        #[cfg(feature = "ssl")]
        {
            let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
            builder.set_certificate_chain_file(cert)?;
            let ft = match key.extension() {
                Some(ext) if ext == "pem" => SslFiletype::PEM,
                _ => SslFiletype::ASN1,
            };
            builder.set_private_key_file(key, ft)?;

            server.bind_ssl(config.net.address, builder)?.start();
        }

        #[cfg(feature = "rust-tls")]
        {
            let mut config = ServerConfig::new(NoClientAuth::new());
            let mut cert_file = BufReader::new(File::open(cert)?);
            let cert_chain = certs(&mut cert_file).or_else(|()| Err(Error::RustTLSNoMsg))?;
            let mut key_file = BufReader::new(File::open(key)?);
            let mut keys = rsa_private_keys(&mut key_file)?;
            config.set_single_cert(cert_chain, keys.remove(0))?;
            server.bind_rustls("127.0.0.1:8443", config)?.start();
        }
    } else {
        server.bind(config.net.address)?.start();
    }

    info!("Started server at {}", config.net.address);
    system.run()?;

    Ok(())
}
