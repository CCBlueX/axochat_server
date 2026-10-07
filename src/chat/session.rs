use super::{
    connect::Connect, ChatServer, ClientPacket, Disconnect, InternalId, ServerPacket,
    ServerPacketId,
};

use log::*;

use actix::*;
use actix_ws as ws;
use std::future::Future;

pub struct Session {
    id: InternalId,
    addr: Addr<ChatServer>,
    ws: ws::Session,
}

impl Session {
    pub fn new(id: InternalId, addr: Addr<ChatServer>, ws: ws::Session) -> Session {
        Session { id, addr, ws }
    }

    // wait, not spawn: keeps outgoing frames in order
    fn send<F>(&self, ctx: &mut Context<Self>, send: impl FnOnce(ws::Session) -> F)
    where
        F: Future<Output = Result<(), ws::Closed>> + 'static,
    {
        ctx.wait(send(self.ws.clone()).into_actor(self).map(|res, _, ctx| {
            if res.is_err() {
                ctx.stop();
            }
        }));
    }
}

impl Actor for Session {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // Updated approach that avoids borrowing issues with ctx
        let addr = self.addr.clone();
        let recipient = ctx.address().recipient();
        
        // wait, not spawn: no frame may be handled before the id is assigned
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

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for Session {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        let msg = match msg {
            Ok(msg) => msg,
            // a client vanishing without a close frame ends the payload early
            Err(ws::ProtocolError::Io(err)) => {
                debug!("Connection `{}` dropped: {}", self.id, err);
                ctx.stop();
                return;
            }
            Err(err) => {
                error!("Error in WebSocket connection: {}", err);
                ctx.stop();
                return;
            }
        };

        debug!("Received message {:?}", msg);
        match msg {
            ws::Message::Ping(msg) => self.send(ctx, |mut ws| async move { ws.pong(&msg).await }),
            ws::Message::Pong(_msg) => {}
            ws::Message::Text(msg) => match serde_json::from_slice::<ServerPacket>(msg.as_ref()) {
                // do_send queues right away, so packets reach the server in order
                Ok(packet) => self.addr.do_send(ServerPacketId {
                    user_id: self.id,
                    packet,
                }),
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
                // a stopping actor no longer polls wait futures, so stop after the close frame
                ctx.wait(
                    self.ws
                        .clone()
                        .close(reason)
                        .into_actor(self)
                        .map(|_, _, ctx| ctx.stop()),
                );
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
        self.send(ctx, |mut ws| async move { ws.text(msg).await });
    }
}
