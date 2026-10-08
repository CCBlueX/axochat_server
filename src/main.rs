mod auth;
mod chat;
mod config;
mod entity;
mod error;
mod ip;
mod message;
mod moderation;
mod store;

use clap::{Parser, Subcommand};
use config::Config;
use error::*;
use log::*;

use actix::*;
use actix_web::{web, App, HttpServer};
use uuid::Uuid;

#[cfg(feature = "rustls-tls")]
use rustls::{
    pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer},
    ServerConfig,
};

#[cfg(feature = "openssl-tls")]
use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};

#[derive(Parser)]
struct Opt {
    #[command(flatten)]
    config: Config,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Starts the axochat server.
    Start,
    /// Generates a JWT which can be used for logging in.
    /// This should only be used for testing.
    /// If you want to generate JWT for non-testing purposes, send a RequestJWT packet to the server.
    Generate {
        name: String,
        uuid: Option<Uuid>,
    },
}

#[actix_web::main]
async fn main() -> Result<()> {
    env_logger::init();

    let Opt { config, command } = Opt::parse();
    match command {
        Command::Start => start_server(config).await,
        Command::Generate { name, uuid } => {
            let auth = match auth::Authenticator::new(&config.auth) {
                Some(auth) => auth,
                None => {
                    eprintln!("Set JWT_SECRET to generate tokens.");
                    return Err(ClientError::NotSupported.into());
                }
            };
            let token = auth.new_token(auth::UserInfo {
                name,
                uuid: uuid.unwrap_or_else(|| Uuid::from_u128(0)),
            })?;
            println!("{}", token);
            Ok(())
        }
    }
}

async fn start_server(config: Config) -> Result<()> {
    // every connection is a file; Docker and systemd default to a soft limit of 1024
    match rlimit::increase_nofile_limit(u64::MAX) {
        Ok(limit) => info!("Open file limit is {}", limit),
        Err(err) => warn!("Could not raise the open file limit: {}", err),
    }

    let db = sea_orm::Database::connect(&config.database.url).await?;
    db.get_schema_registry("axochat::entity::*").sync(&db).await?;
    let store = store::Store::new(db).start();

    let server_config = config.clone();
    let server = chat::ChatServer::new(server_config, store).start();

    let server_data = web::Data::new(server);
    let real_ip = web::Data::new(ip::RealIp::new(
        &config.net.trusted_proxies,
        config.net.real_ip_header.clone(),
    ));
    let address = config.net.address.to_string();

    let mut server = HttpServer::new(move || {
        App::new()
            .app_data(server_data.clone())
            .app_data(real_ip.clone())
            .service(web::resource("/ws").to(chat::chat_route))
    });

    if let (Some(cert), Some(key)) = (config.net.cert_file, config.net.key_file) {
        #[cfg(all(feature = "openssl-tls", feature = "rustls-tls"))]
        {
            compile_error!("Can't enable both the `openssl-tls` and the `rustls-tls` feature.")
        }

        #[cfg(feature = "openssl-tls")]
        {
            let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
            builder.set_certificate_chain_file(&cert)?;
            let ft = match key.extension() {
                Some(ext) if ext == "pem" => SslFiletype::PEM,
                _ => SslFiletype::ASN1,
            };
            builder.set_private_key_file(&key, ft)?;

            server = server.bind_openssl(address, builder)?;
        }

        #[cfg(feature = "rustls-tls")]
        {
            info!("Loading TLS certificate from {:?} and key from {:?}", cert, key);
            
            let cert_chain = CertificateDer::pem_file_iter(&cert)
                .and_then(|certs| certs.collect::<std::result::Result<Vec<_>, _>>())
                .map_err(|_| Error::RustTLSNoMsg)?;
            let key = PrivateKeyDer::from_pem_file(&key).map_err(|_| Error::RustTLSNoMsg)?;

            let config = ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(cert_chain, key)
                .map_err(|_| Error::RustTLSNoMsg)?;

            server = server.bind_rustls_0_23(address, config)?;
        }

        #[cfg(not(any(feature = "openssl-tls", feature = "rustls-tls")))]
        {
            server = server.bind(address)?;
        }
    } else {
        server = server.bind(address)?;
    }

    info!("Started server at {}", config.net.address);
    server.run().await?;
    Ok(())
}
