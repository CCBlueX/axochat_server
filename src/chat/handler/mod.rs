mod ban;
mod jwt;
mod message;
mod mojang;

use super::{ChatServer, ClientPacket, Id, ServerPacket, ServerPacketId};

use actix::*;

impl Handler<ServerPacketId> for ChatServer {
    type Result = ();

    fn handle(
        &mut self,
        ServerPacketId { user_id, packet }: ServerPacketId,
        ctx: &mut Context<Self>,
    ) {
        match packet {
            ServerPacket::RequestMojangInfo => {
                self.handle_request_mojang_info(user_id);
            }
            ServerPacket::LoginMojang(info) => {
                self.login_mojang(user_id, info, ctx);
            }
            ServerPacket::RequestJWT => {
                self.handle_request_jwt(user_id);
            }
            ServerPacket::LoginJWT {
                token,
                anonymous,
                allow_messages,
            } => {
                self.handle_login_jwt(user_id, &token, anonymous, allow_messages);
            }
            ServerPacket::Message { content } => self.handle_message(user_id, content),
            ServerPacket::PrivateMessage { receiver, content } => {
                self.handle_private_message(user_id, receiver, content);
            }
            ServerPacket::BanUser(to_ban) => {
                self.ban_user(user_id, to_ban);
            }
            ServerPacket::UnbanUser(to_ban) => {
                self.unban_user(user_id, to_ban);
            }
        }
    }
}
