use super::{
    connect::Connect, ChatServer, ClientPacket, Disconnect, InternalId, ServerPacket,
    ServerPacketId,
};

use log::*;

use actix::*;
use actix_web_actors::ws;

pub struct Session {
    id: InternalId,
    addr: Addr<ChatServer>,
}

impl Session {
    pub fn new(id: InternalId, addr: Addr<ChatServer>) -> Session {
        Session { id, addr }
    }
}

impl Actor for Session {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // Updated approach that avoids borrowing issues with ctx
        let addr = self.addr.clone();
        let recipient = ctx.address().recipient();
        
        // Use a proper async spawn that avoids borrowing ctx in the async block
        ctx.wait(async move {
                addr.send(Connect::new(recipient)).await
            }
            .into_actor(self)
            .map(|res, actor, _ctx| {
                match res {
                    Ok(id) => {
                        actor.id = id;
                    }
                    Err(err) => {
                        warn!("Could not accept connection: {}", err);
                    }
                }
            })
        );
    }

    fn stopping(&mut self, _ctx: &mut Self::Context) -> Running {
        self.addr.do_send(Disconnect { id: self.id });
        Running::Stop
    }
}

// Updated StreamHandler for actix-web-actors 4.x
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for Session {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        let msg = match msg {
            Ok(msg) => msg,
            Err(err) => {
                error!("Error in WebSocket connection: {}", err);
                ctx.stop();
                return;
            }
        };

        debug!("Received message {:?}", msg);
        match msg {
            ws::Message::Ping(msg) => ctx.pong(&msg),
            ws::Message::Pong(_msg) => {}
            ws::Message::Text(msg) => match serde_json::from_slice::<ServerPacket>(msg.as_ref()) {
                Ok(packet) => {
                    let addr = self.addr.clone();
                    let user_id = self.id;
                    ctx.wait(async move {
                            addr.send(ServerPacketId {
                                user_id,
                                packet,
                            }).await
                        }
                        .into_actor(self)
                        .map(|res, _, _| {
                            if let Err(err) = res {
                                warn!("Could not deliver packet: {}", err);
                            }
                        })
                    );
                }
                Err(err) => {
                    warn!("Could not decode packet: {}", err);
                }
            },
            ws::Message::Binary(_msg) => {
                warn!("Can't decode binary messages.");
            }
            ws::Message::Close(reason) => {
                // Fix borrowing issue with reason by cloning it
                if let Some(ref reason) = reason {
                    info!(
                        "Connection `{}` closed; code: {:?}, reason: {:?}",
                        self.id, reason.code, reason.description
                    );
                } else {
                    info!("Connection `{}` closed.", self.id);
                }
                ctx.close(reason);
                ctx.stop();
            }
            ws::Message::Continuation(_) => {
                warn!("Continuation frames are not supported.");
                ctx.stop();
            }
            ws::Message::Nop => {}
        }
    }
}

impl Handler<ClientPacket> for Session {
    type Result = ();

    fn handle(&mut self, msg: ClientPacket, ctx: &mut Self::Context) {
        let msg = serde_json::to_string(&msg).expect("could not encode message");
        ctx.text(msg);
    }
}
