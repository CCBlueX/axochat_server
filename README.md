# AxoChat Server
A generic server for chat features in Minecraft modifications utilizing the Mojang authentication scheme and WebSockets. LiquidBounce employs it for its global chat feature, which allows users to communicate with other people using the client regardless of the current server.

## Implementation
A specification of the protocol used can be found [here](PROTOCOL.md).

## Usage
The server is configured through environment variables, see [.env.example](.env.example).

```sh
cargo run --release -- start
```

It needs a MySQL or MariaDB database; tables are created on startup.
Mutes from an old `banned.txt` move into the database with `axochat import-bans banned.txt`.
