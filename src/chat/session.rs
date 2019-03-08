use super::{
    connect::Connect, ClientPacket, Disconnect, Id, ServerPacket, ServerPacketId, ServerState,
};

use log::*;

use actix::*;
use actix_web::ws;

pub struct Session {
    id: Id,
}

impl Session {
    pub fn new(id: Id) -> Session {
        Session { id }
    }
}

impl Actor for Session {
    type Context = ws::WebsocketContext<Self, ServerState>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.state()
            .addr
            .send(Connect::new(ctx.address().recipient()))
            .into_actor(self)
            .then(|res, actor, ctx| {
                match res {
                    Ok(id) => {
                        actor.id = id;
                    }
                    Err(err) => {
                        warn!("Could not accept connection: {}", err);
                        ctx.stop();
                    }
                }
                fut::ok(())
            })
            .wait(ctx)
    }

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        ctx.state().addr.do_send(Disconnect { id: self.id });
        Running::Stop
    }
}

impl StreamHandler<ws::Message, ws::ProtocolError> for Session {
    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
        debug!("Received message {:?}", msg);
        match msg {
            ws::Message::Ping(msg) => ctx.pong(&msg),
            ws::Message::Pong(_msg) => {}
            ws::Message::Text(msg) => match serde_json::from_slice::<ServerPacket>(msg.as_ref()) {
                Ok(packet) => ctx
                    .state()
                    .addr
                    .send(ServerPacketId {
                        user_id: self.id,
                        packet,
                    })
                    .into_actor(self)
                    .map_err(|err, _actor, _ctx| {
                        warn!("Could not decode packet: {}", err);
                    })
                    .wait(ctx),
                Err(err) => {
                    warn!("Could not decode packet: {}", err);
                }
            },
            ws::Message::Binary(_msg) => {
                warn!("Can't decode binary messages.");
            }
            ws::Message::Close(Some(reason)) => {
                info!(
                    "Connection `{}` closed; code: {:?}, reason: {:?}",
                    self.id, reason.code, reason.description
                );
                ctx.stop();
            }
            ws::Message::Close(None) => {
                info!("Connection `{}` closed.", self.id);
                ctx.stop();
            }
        }
    }
}

impl Handler<ClientPacket> for Session {
    type Result = ();

    fn handle(&mut self, msg: ClientPacket, ctx: &mut Self::Context) {
        let bytes = serde_json::to_vec(&msg).expect("could not encode message");
        ctx.text(bytes);
    }
}
