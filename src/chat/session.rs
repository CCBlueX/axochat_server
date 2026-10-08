use super::{
    connect::Connect, ChatServer, ClientPacket, Disconnect, InternalId, ServerPacket,
    ServerPacketId,
};

use log::*;

use actix::*;
use actix_ws as ws;
use std::future::Future;
use std::time::{Duration, Instant};

const PING_INTERVAL: Duration = Duration::from_secs(30);
const PONG_TIMEOUT: Duration = Duration::from_secs(90);

pub struct Session {
    id: InternalId,
    addr: Addr<ChatServer>,
    ws: ws::Session,
    // old clients never answer pings, so the timeout only applies after the first pong
    last_pong: Option<Instant>,
}

impl Session {
    pub fn new(id: InternalId, addr: Addr<ChatServer>, ws: ws::Session) -> Session {
        Session {
            id,
            addr,
            ws,
            last_pong: None,
        }
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

    fn close(&self, ctx: &mut Context<Self>, reason: Option<ws::CloseReason>) {
        // a stopping actor does not poll wait futures, so stop after the close frame
        ctx.wait(
            self.ws
                .clone()
                .close(reason)
                .into_actor(self)
                .map(|_, _, ctx| ctx.stop()),
        );
    }
}

impl Actor for Session {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let addr = self.addr.clone();
        let recipient = ctx.address().recipient();
        
        // wait, not spawn: no frame may be handled before the id is assigned
        ctx.wait(async move {
                addr.send(Connect::new(recipient)).await
            }
            .into_actor(self)
            .map(|res, actor, ctx| {
                match res {
                    Ok(id) => {
                        actor.id = id;
                    }
                    Err(err) => {
                        warn!("Could not accept connection: {}", err);
                        ctx.stop();
                    }
                }
            })
        );

        ctx.run_interval(PING_INTERVAL, |actor, ctx| {
            if actor.last_pong.is_some_and(|last| last.elapsed() > PONG_TIMEOUT) {
                info!("Connection `{}` timed out.", actor.id);
                actor.close(ctx, Some(ws::CloseCode::Away.into()));
                return;
            }
            actor.send(ctx, |mut ws| async move { ws.ping(b"").await });
        });
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
            ws::Message::Pong(_msg) => self.last_pong = Some(Instant::now()),
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
                if let Some(ref reason) = reason {
                    info!(
                        "Connection `{}` closed; code: {:?}, reason: {:?}",
                        self.id, reason.code, reason.description
                    );
                } else {
                    info!("Connection `{}` closed.", self.id);
                }
                self.close(ctx, reason);
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
