mod auth;
mod chat;
mod config;
mod error;
mod message;
mod moderation;

use config::Config;
use error::*;
use clap::Parser;
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
enum Opt {
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

    let config = config::read_config()?;
    debug!("Read configuration file: {:?}", config);

    let opt = Opt::parse();
    match opt {
        Opt::Start => start_server(config).await,
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

async fn start_server(config: Config) -> Result<()> {
    let server_config = config.clone();
    let server = chat::ChatServer::new(server_config).start();

    let server_data = web::Data::new(server);
    let address = config.net.address.to_string();

    let mut server = HttpServer::new(move || {
        App::new()
            .app_data(server_data.clone())
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
