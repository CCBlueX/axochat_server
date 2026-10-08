//! The whole server against MariaDB and a mocked Mojang and Service API; skipped without `DATABASE_URL`.

use crate::{active_punishments, chat, ip, open_store, store, Opt};

use actix::Actor;
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer};
use awc::ws;
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Duration;

const NOTCH: &str = "069a79f444e94726a5befca90e38aaf5";

fn uuid_of(name: &str) -> String {
    if name.eq_ignore_ascii_case("notch") {
        return NOTCH.into();
    }
    let mut hasher = DefaultHasher::new();
    name.to_lowercase().hash(&mut hasher);
    let hash = hasher.finish();
    format!("{:016x}{:016x}", hash, hash.rotate_left(17))
}

async fn has_joined(query: web::Query<std::collections::HashMap<String, String>>) -> HttpResponse {
    let name = &query["username"];
    let canonical = if name.eq_ignore_ascii_case("notch") { "Notch" } else { name };
    HttpResponse::Ok().json(json!({ "id": uuid_of(name), "name": canonical, "properties": [] }))
}

/// Tokens look like `tok:<sub>:<nickname>:<roles>`.
async fn oauth_user(req: HttpRequest) -> HttpResponse {
    let token = req
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    match token.split(':').collect::<Vec<_>>()[..] {
        ["tok", sub, nickname, roles] => {
            let roles: Vec<&str> = roles.split(',').filter(|role| !role.is_empty()).collect();
            HttpResponse::Ok().json(json!({
                "user_id": sub, "email": "e2e@example.com", "name": null, "nickname": nickname,
                "roles": roles, "groups": roles, "premium": false, "is_admin": false, "minecraft_uuid": null
            }))
        }
        _ => HttpResponse::Unauthorized().finish(),
    }
}

async fn roles() -> HttpResponse {
    HttpResponse::Ok().json(json!([
        { "id": "staff", "display_name": "Staff", "is_staff": true, "purchase_only": false, "discord_role_id": null },
        { "id": "premium", "display_name": "Premium", "is_staff": false, "purchase_only": true, "discord_role_id": null }
    ]))
}

fn mock() -> String {
    let server = HttpServer::new(|| {
        App::new()
            .route("/session/minecraft/hasJoined", web::get().to(has_joined))
            .route("/api/v3/oauth/user", web::get().to(oauth_user))
            .route("/api/v2/user/roles", web::get().to(roles))
            .route("/api/v2/user/by-minecraft/{uuid}", web::get().to(HttpResponse::NotFound))
    })
    .workers(1)
    .bind("127.0.0.1:0")
    .unwrap();
    let address = server.addrs()[0];
    actix_web::rt::spawn(server.run());
    format!("http://{}", address)
}

async fn axochat(database: &str) -> String {
    let mock = mock();
    let opt = Opt::try_parse_from([
        "axochat",
        "--database-url",
        database,
        "--api-url",
        &mock,
        "--api-token",
        "service",
        "--mojang-session-url",
        &mock,
        "start",
    ])
    .unwrap();
    let config = opt.config;
    let store = open_store(&config).await.unwrap();
    let punishments = active_punishments(&store).await.unwrap();
    let state = store.send(store::LoadState).await.unwrap().unwrap();
    let server = web::Data::new(chat::ChatServer::new(config, store, punishments, state).start());
    let real_ip = web::Data::new(ip::RealIp::new(&[], "CF-Connecting-IP".into()));

    let http = HttpServer::new(move || {
        App::new()
            .app_data(server.clone())
            .app_data(real_ip.clone())
            .service(web::resource("/ws").to(chat::chat_route))
    })
    .workers(1)
    .bind("127.0.0.1:0")
    .unwrap();
    let address = http.addrs()[0];
    actix_web::rt::spawn(http.run());
    // roles load in the background
    actix_web::rt::time::sleep(Duration::from_millis(300)).await;
    format!("ws://{}/ws", address)
}

struct Client {
    framed: actix_codec::Framed<awc::BoxedSocket, ws::Codec>,
}

impl Client {
    async fn connect(url: &str) -> Client {
        let (_, framed) = awc::Client::new().ws(url).connect().await.unwrap();
        Client { framed }
    }

    async fn send(&mut self, packet: Value) {
        self.framed.send(ws::Message::Text(packet.to_string().into())).await.unwrap();
    }

    async fn next_any(&mut self) -> Value {
        loop {
            let frame = actix_web::rt::time::timeout(Duration::from_secs(5), self.framed.next())
                .await
                .expect("no packet within 5 s")
                .unwrap()
                .unwrap();
            if let ws::Frame::Text(text) = frame {
                return serde_json::from_slice(&text).unwrap();
            }
        }
    }

    async fn closed(&mut self) -> bool {
        let frame = actix_web::rt::time::timeout(Duration::from_secs(5), self.framed.next()).await;
        matches!(frame, Ok(None | Some(Ok(ws::Frame::Close(_)))))
    }

    async fn next(&mut self, name: &str) -> Value {
        loop {
            let packet = self.next_any().await;
            if packet["m"] == name {
                return packet;
            }
        }
    }

    async fn mojang(&mut self, name: &str) {
        self.send(json!({ "m": "RequestMojangInfo" })).await;
        self.next("MojangInfo").await;
        self.send(json!({ "m": "LoginMojang", "c": { "name": name, "uuid": uuid_of(name), "allow_messages": true } }))
            .await;
    }

    /// Logs in on v2, returning the packets up to `Success`.
    async fn account(&mut self, sub: &str, nickname: &str, roles: &str) -> Vec<Value> {
        self.send(json!({ "m": "Hello", "c": { "protocol": 2 } })).await;
        assert_eq!(self.next_any().await, json!({ "m": "Hello", "c": { "protocol": 2 } }));
        let token = format!("tok:{}:{}:{}", sub, nickname, roles);
        self.send(json!({ "m": "LoginAccount", "c": { "token": token, "allow_messages": true } })).await;
        let mut packets = Vec::new();
        loop {
            let packet = self.next_any().await;
            let done = packet["m"] == "Success";
            packets.push(packet);
            if done {
                return packets;
            }
        }
    }
}

fn run_id() -> String {
    let mut hasher = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut hasher);
    format!("{:06x}", hasher.finish() & 0xff_ffff)
}

#[actix_web::test]
async fn protocol() {
    let Ok(database) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set, skipping");
        return;
    };
    let url = axochat(&database).await;
    let run = run_id();

    // v1, byte for byte as old clients expect it
    let mut old = Client::connect(&url).await;
    old.mojang("notch").await;
    assert_eq!(old.next_any().await, json!({ "m": "Success", "c": { "reason": "Login" } }));
    old.send(json!({ "m": "Message", "c": { "content": "bad §c" } })).await;
    assert_eq!(old.next_any().await, json!({ "m": "Error", "c": { "message": "InvalidCharacter" } }));
    old.send(json!({ "m": "LoginJWT", "c": { "token": "x", "allow_messages": true } })).await;
    assert_eq!(old.next_any().await, json!({ "m": "Error", "c": { "message": "NotSupported" } }));

    // v2 login snapshots, then Success
    let mut alice = Client::connect(&url).await;
    let login = alice.account(&format!("alice-{}", run), &format!("Alice{}", run), "premium").await;
    let names: Vec<&str> = login.iter().map(|packet| packet["m"].as_str().unwrap()).collect();
    assert_eq!(names, ["Welcome", "Settings", "Friends", "Blocks", "Groups", "Party", "Success"]);
    assert_eq!(login[0]["c"]["user"]["roles"], json!([{ "id": "premium", "name": "Premium", "staff": false }]));
    assert_eq!(login[0]["c"]["user"]["highlight"], json!(true));

    // global chat reaches both protocols in their own shape
    alice.send(json!({ "m": "ChatMessage", "c": { "channel": "global", "content": "§ahi 😀" } })).await;
    let v1 = old.next("Message").await;
    assert_eq!(v1["c"]["content"], "§ahi 😀");
    assert_eq!(v1["c"]["author_info"]["name"], format!("Alice{}", run));
    let v2 = alice.next("ChatMessage").await;
    assert_eq!(v2["c"]["channel"], "global");
    assert!(v2["c"]["id"].is_u64());

    // direct messages are between accounts
    old.send(json!({ "m": "PrivateMessage", "c": { "receiver": format!("alice{}", run), "content": "psst" } })).await;
    assert_eq!(old.next_any().await, json!({ "m": "Error", "c": { "message": "PrivateMessageNotAccepted" } }));
    let mut bob = Client::connect(&url).await;
    bob.account(&format!("bob-{}", run), &format!("Bob{}", run), "").await;
    bob.send(json!({ "m": "ChatMessage", "c": { "channel": "user/notch", "content": "psst" } })).await;
    assert_eq!(bob.next("Error").await["c"]["message"], "PrivateMessageNotAccepted");
    bob.send(json!({ "m": "ChatMessage", "c": { "channel": format!("user/alice{}", run), "content": "hi alice" } })).await;
    let direct = alice.next("ChatMessage").await;
    assert_eq!(direct["c"]["channel"], format!("user/{}", direct["c"]["author"]["id"].as_str().unwrap()));
    assert_eq!(direct["c"]["author"]["minecraft"], Value::Null);
    bob.next("ChatMessage").await;

    // an account session proves the Minecraft account it plays on
    alice.send(json!({ "m": "RequestMojangInfo" })).await;
    alice.next("MojangInfo").await;
    let player = format!("alice_mc{}", run);
    alice.send(json!({ "m": "LoginMojang", "c": { "name": player, "uuid": uuid_of(&player), "allow_messages": true } }))
        .await;
    assert_eq!(alice.next("Success").await["c"]["reason"], "Minecraft");
    alice.send(json!({ "m": "ChatMessage", "c": { "channel": format!("user/bob{}", run), "content": "hey" } })).await;
    let author = bob.next("ChatMessage").await["c"]["author"].clone();
    assert_eq!(author["name"], format!("Alice{}", run));
    assert_eq!(author["minecraft"]["name"], player);
    assert_eq!(author["uuid"], author["minecraft"]["uuid"]);

    // an invite to a name nobody has looks like any other
    alice.send(json!({ "m": "Party", "c": { "action": "invite", "user": format!("Nobody{}", run) } })).await;
    loop {
        let packet = alice.next_any().await;
        assert_ne!(packet["m"], "Error");
        if packet["m"] == "Party" {
            break;
        }
    }

    // a party in the same world
    alice.send(json!({ "m": "Party", "c": { "action": "invite", "user": format!("Bob{}", run) } })).await;
    let invite = bob.next("PartyInvite").await;
    bob.send(json!({ "m": "Party", "c": { "action": "accept", "party": invite["c"]["party"] } })).await;
    let world = json!({ "dimension": "minecraft:overworld", "seed": "-4172144997902289642", "age": 1000000 });
    alice.send(json!({ "m": "Location", "c": { "server": "mc.example.org", "world": world, "player": null } })).await;
    bob.send(json!({ "m": "Location", "c": { "server": "example.org", "world": world, "player": null } })).await;
    loop {
        let party = bob.next("Party").await;
        let members = party["c"]["party"]["members"].as_array().unwrap().clone();
        let alice = members.iter().find(|member| member["user"]["name"] == format!("Alice{}", run));
        if alice.is_some_and(|alice| alice["relation"] == "world") {
            break;
        }
    }
    alice.send(json!({ "m": "PartyState", "c": { "position": { "x": 1.0, "y": 2.0, "z": 3.0, "yaw": 0.0, "pitch": 0.0, "dimension": null } } })).await;
    assert_eq!(bob.next("PartyMemberState").await["c"]["position"]["z"], json!(3.0));

    // a LAN server is shared with members on the same network, and the leader sees it was sent
    alice.send(json!({ "m": "Location", "c": { "server": "localhost:25570", "world": world, "player": null } })).await;
    alice.send(json!({ "m": "Party", "c": { "action": "warp" } })).await;
    assert_eq!(bob.next("PartyWarp").await["c"]["server"], "localhost:25570");
    assert_eq!(alice.next("PartyWarp").await["c"]["server"], "localhost:25570");

    // accepting another invite switches parties
    let mut carol = Client::connect(&url).await;
    carol.account(&format!("carol-{}", run), &format!("Carol{}", run), "").await;
    carol.send(json!({ "m": "Party", "c": { "action": "invite", "user": format!("Bob{}", run) } })).await;
    let invite = bob.next("PartyInvite").await;
    bob.send(json!({ "m": "Party", "c": { "action": "accept", "party": invite["c"]["party"] } })).await;
    loop {
        let party = bob.next("Party").await;
        if party["c"]["party"]["id"] == invite["c"]["party"] {
            break;
        }
    }

    // sockets that never log in are capped per address
    let mut idle = Vec::new();
    for _ in 0..16 {
        let mut client = Client::connect(&url).await;
        client.send(json!({ "m": "Hello", "c": { "protocol": 2 } })).await;
        client.next("Hello").await;
        idle.push(client);
    }
    assert!(Client::connect(&url).await.closed().await, "the 17th is refused");
}
