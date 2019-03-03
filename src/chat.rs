use crate::error::*;
use crate::config::Config;
use log::*;
use actix_web::{ws, actix::{Addr, Actor, StreamHandler}, HttpRequest, HttpResponse};

pub fn chat_route(req: &HttpRequest<ServerState>) -> actix_web::Result<HttpResponse>{
    ws::start(
        req,
        Session,
    )
}

#[derive(Clone)]
pub struct ServerState;

pub struct Session;

impl Actor for Session {
    type Context = ws::WebsocketContext<Self, ServerState>;
}

impl StreamHandler<ws::Message, ws::ProtocolError> for Session {
    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
        debug!("Received message {:?}", msg);
    }
}
