# The AxoChat protocol
The AxoChat protocol is based on websockets.
All packets are sent to the `/ws` endpoint.

# Structures
Some structures are used multiple times; these are described here.

## Id
An `Id` is a SHA256 hashed uuid of a user.
When generating, the uuid must be treated as its raw bytes.
It is encoded as a hex string.

### Example
```json
"14233838f75ace1c6cc2a9d39c18e02157b84282124708edfead45dd9bb62621"
```

## UserInfo
`UserInfo` is just the `name` and `uuid` of a user.

### Example
```json
{
    "name": "Notch",
    "uuid": "069a79f4-44e9-4726-a5be-fca90e38aaf5"
}
```

# Packets
Packets are sent in websocket `text` messages encoded as JSON objects.
They all have a structure like that, with `c` being optional:
```json
{
    "m": "Name",
    "c": {
        "...": "...",
        "...": false
    }
}
```

Not every packet has a body:
```json
{
    "m": "Name"
}
```

## Client
Client Packets are received by the client.

### Error
This packet may be sent at any time,
but is usually a response to a failed action of the client.

#### Example
```json
{
    "m": "Error",
    "c": {
        "message": "LoginFailed"
    }
}
```

### Message
This packet will be sent to every authenticated client,
if another client successfully [sent a message](#message) to the server.

`author_id` is an [Id](#id).
`author_info` is optional and described in detail in [UserInfo](#userinfo).
`content` is any message fitting the validation scheme of the server.

#### Example
```json
{
    "m": "Message",
    "c": {
        "author_id": "14233838f75ace1c6cc2a9d39c18e02157b84282124708edfead45dd9bb62621",
        "author_info": {
            "name": "Notch",
            "uuid": "069a79f4-44e9-4726-a5be-fca90e38aaf5"
        },
        "content": "Hello, World!"
    }
}
```

### MojangInfo
After the client sent the server a [RequestMojangInfo](#requestmojanginfo)
packet, the server will provide the client with a *session hash*.
A session hash is synonymous with a *server id* in the context of
[authentication with mojang](https://wiki.vg/Protocol_Encryption#Authentication).
The client has to send a [LoginMojang](#loginmojang) packet to the server
after authenticating itself with mojang.

#### Example
```json
{
    "m": "MojangInfo",
    "c": {
        "session_hash": "88e16a1019277b15d58faf0541e11910eb756f6"
    }
}
```

### NewJWT
After the client sent the server a [RequestJWT](#requestjwt)
packet, the server will provide the client with json web token.
This token can be used in the [LoginJWT](#loginjwt) packet.

#### Example
```json
{
    "m": "NewJWT",
    "c": {
        "token": "VGhpcyBjb3VsZCBiZSBhIGpzb24gd2ViIHRva2VuLCBidXQgaXQgaXNuJ3QK"
    }
}
```

### PrivateMessage
This packet will be sent to a authenticated client with `allow_messages` turned on,
if another client successfully [sent a private message](#privatemessage)
to the server with the [id](#id).

`author_id` is an [Id](#id).
`author_info` is optional and described in detail in [UserInfo](#userinfo).
`content` is any message fitting the validation scheme of the server.

#### Example
```json
{
    "m": "PrivateMessage",
    "c": {
        "author_id": "14233838f75ace1c6cc2a9d39c18e02157b84282124708edfead45dd9bb62621",
        "author_info": {
            "name": "Notch",
            "uuid": "069a79f4-44e9-4726-a5be-fca90e38aaf5"
        },
        "content": "Hello, User!"
    }
}
```

### Success
This packet is sent after either
[LoginMojang](#loginmojang), [LoginJWT](#loginjwt),
[BanUser](#banuser) or [UnbanUser](#unbanuser)
were processed successfully.

#### Example
```json
{
    "m": "Success"
}
```

## Server
Server Packets are received by the server.

### BanUser
A client can send this packet to ban other users from using this chat.

`user` is an [Id](#id).

#### Example
```json
{
    "m": "BanUser",
    "c": {
        "user": "40ae0781f85042de8108a323228a3a2488a7fa84d6d26a023718941f01c5f44c"
    }
}
```

### LoginJWT
To login using a json web token, the client has to send a `LoginJWT` packet.
it will send [Success](#success) if the login was successful.

`token` can be retrieved by sending [RequestJWT](#requestjwt) on an already
authenticated connection.
If `anonymous` is true, other clients will never know `name`.
If `allow_messages` is true, other clients may send private messages
to this client.

#### Example
```json
{
    "m": "LoginJWT",
    "c": {
        "token": "VGhpcyBjb3VsZCBiZSBhIGpzb24gd2ViIHRva2VuLCBidXQgaXQgaXNuJ3QK",
        "anonymous": false,
        "allow_messages": true,
    }
}
```

### LoginMojang
After the client received a [MojangInfo](#mojanginfo) packet
and authenticating itself with mojang,
it has to send a `LoginMojang` packet to the server.
After the server receives a `LoginMojang` packet,
it will send [Success](#success) if the login was successful.

`name` needs to be associated with the uuid.
`uuid` is not guaranteed to be hyphenated.
If `anonymous` is true, other clients will never know `name`.
If `allow_messages` is true, other clients may send private messages
to this client.

#### Example
```json
{
    "m": "RequestMojangInfo",
    "c": {
        "name": "Notch",
        "uuid": "069a79f4-44e9-4726-a5be-fca90e38aaf5",
        "anonymous": false,
        "allow_messages": true
    }
}
```

### Message
The `content` of this packet will be sent to every client
as [Message](#message) if it fits the validation scheme.

#### Example
```json
{
    "m": "Message",
    "c": {
        "content": "Hello, World!"
    }
}
```

### PrivateMessage
The `content` of this packet will be sent to the specified client
as [PrivateMessage](#privatemessage) if it fits the validation scheme.

`receiver` is an [Id](#id).

#### Example
```json
{
    "m": "PrivateMessage",
    "c": {
        "content": "Hello, User!",
        "receiver": "7954cfd733c9abcba682565195b1f8215b07f74fb180923ad156ff73821cb3f2"
    }
}
```

### RequestJWT
To login using [LoginJWT](#loginjwt), a client needs to own a json web token.
This token can be retrieved by sending `RequestJWT` as an already authenticated
client to the server.
The server will send a [NewJWT](#newjwt) packet to the client.

#### Example
```json
{
    "m": "RequestJWT"
}
```

### RequestMojangInfo
To login via mojang, the client has to send a `RequestMojangInfo` packet.
The server will then send a [MojangInfo](#mojanginfo) to the client.

This packet has no body.

#### Example
```json
{
    "m": "RequestMojangInfo"
}
```

### UnbanUser
A client can send this packet to unban other users.

`user` is an [Id](#id).

#### Example
```json
{
    "m": "UnbanUser",
    "c": {
        "user": "584b4914b5f6fdf686398be186799633a3149d87ad55ff82c91020599ddc7148"
    }
}
```
