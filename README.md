# AxoChat
A chat server for Minecraft modifications over WebSockets. Users log in with their Minecraft account or their LiquidBounce Account. LiquidBounce uses it for its global chat, direct messages, friends, group chats and parties, regardless of the server a player is on.

## Implementation
A specification of the protocol used can be found [here](PROTOCOL.md).

## Usage
The server is configured through environment variables, see [.env.example](.env.example).

```sh
cargo run --release -- start
```

It needs a MySQL or MariaDB database; tables are created on startup.
Mutes from an old `banned.txt` move into the database with `axochat import-bans banned.txt`.

`cargo test` also runs the server end to end when `DATABASE_URL` points at an empty database.

## Kotlin client
[client](client) is the JVM client LiquidBounce builds on, published as `net.ccbluex:axochat-client` to `https://maven.ccbluex.net/releases` (snapshots of master to `/snapshots`).

```kotlin
val session = AxochatClient(URI("ws://127.0.0.1:8080/ws")).connect()
session.negotiate()
session.loginAccount(accessToken, allowMessages = true)
session.packets.collect { packet -> /* Clientbound.ChatMessage, Clientbound.Party, ... */ }
```

Releases come from GitHub releases tagged `client-v<version>`. `AXOCHAT_URL=ws://127.0.0.1:8080/ws ./gradlew test` runs it against a local server.
