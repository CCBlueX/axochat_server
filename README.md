# AxoChat Server
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
