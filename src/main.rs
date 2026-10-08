mod api;
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
use rand::{rngs::SysRng, SeedableRng};
use rand_hc::Hc128Rng;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
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
    /// Mutes the Minecraft UUIDs of a line-separated file, like the old `banned.txt`.
    ImportBans { file: PathBuf },
}

#[actix_web::main]
async fn main() -> Result<()> {
    env_logger::init();

    let Opt { config, command } = Opt::parse();
    match command {
        Command::Start => start_server(config).await,
        Command::ImportBans { file } => import_bans(config, &file).await,
    }
}

async fn open_store(config: &Config) -> Result<Addr<store::Store>> {
    let db = sea_orm::Database::connect(&config.database.url).await?;
    db.get_schema_registry("axochat::entity::*").sync(&db).await?;
    Ok(store::Store::new(db).start())
}

fn mailbox(err: MailboxError) -> Error {
    Error::IO { source: std::io::Error::other(err) }
}

async fn active_punishments(store: &Addr<store::Store>) -> Result<Vec<moderation::Punishment>> {
    let models = store
        .send(store::LoadPunishments { now: chat::now_ms() })
        .await
        .map_err(mailbox)??;
    Ok(models.into_iter().map(moderation::Punishment::from_model).collect())
}

async fn import_bans(config: Config, file: &Path) -> Result<()> {
    let store = open_store(&config).await?;
    let muted: HashSet<_> = active_punishments(&store)
        .await?
        .into_iter()
        .filter(|punishment| punishment.kind == entity::punishment::Kind::Mute)
        .filter_map(|punishment| punishment.user)
        .collect();
    let mut rng = Hc128Rng::try_from_rng(&mut SysRng).expect("could not initialize hc128 rng");

    let mut imported = 0;
    for line in std::fs::read_to_string(file)?.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let uuid: Uuid = line.parse()?;
        let now = chat::now_ms();
        let create = Some((chat::new_id(&mut rng), now));
        let targets = store
            .send(store::MinecraftTargets { uuid, create })
            .await
            .map_err(mailbox)??;
        for target in targets.into_iter().filter(|target| !muted.contains(&target.id)) {
            let punishment = moderation::Punishment {
                id: chat::new_id(&mut rng),
                kind: entity::punishment::Kind::Mute,
                user: Some(target.id),
                ip: None,
                reason: String::new(),
                issued_by: None,
                created_at: now,
                expires_at: None,
            };
            store
                .send(store::Persist(vec![store::Write::Punish(punishment.to_model())]))
                .await
                .map_err(mailbox)?;
            imported += 1;
        }
    }
    println!("Muted {} users.", imported);
    Ok(())
}

async fn start_server(config: Config) -> Result<()> {
    // every connection is a file; Docker and systemd default to a soft limit of 1024
    match rlimit::increase_nofile_limit(u64::MAX) {
        Ok(limit) => info!("Open file limit is {}", limit),
        Err(err) => warn!("Could not raise the open file limit: {}", err),
    }

    let store = open_store(&config).await?;
    let punishments = active_punishments(&store).await?;
    let state = store.send(store::LoadState).await.map_err(mailbox)??;

    let server_config = config.clone();
    let server = chat::ChatServer::new(server_config, store, punishments, state).start();

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
