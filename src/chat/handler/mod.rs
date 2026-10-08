mod account;
mod punish;
mod social;
mod count;
mod hello;
mod jwt;
mod login;
mod message;
mod mojang;

use super::{ChatServer, ServerPacket, ServerPacketId};

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
            ServerPacket::LoginAccount { token, allow_messages } => {
                self.login_account(user_id, token, allow_messages, ctx);
            }
            ServerPacket::RequestJWT | ServerPacket::LoginJWT(_) => {
                self.handle_jwt(user_id);
            }
            ServerPacket::Message { content } => self.handle_message(user_id, content),
            ServerPacket::ChatMessage { channel, content } => self.handle_chat_message(user_id, channel, content),
            ServerPacket::PrivateMessage { receiver, content } => {
                self.handle_private_message(user_id, receiver, content);
            }
            ServerPacket::BanUser { user } => {
                self.ban_user(user_id, user, ctx);
            }
            ServerPacket::UnbanUser { user } => {
                self.unban_user(user_id, user, ctx);
            }
            ServerPacket::Punish {
                user,
                ip,
                kind,
                duration,
                reason,
                include_ip,
            } => {
                self.handle_punish(user_id, user, ip, kind, duration, reason, include_ip, ctx);
            }
            ServerPacket::Pardon { user, ip } => {
                self.handle_pardon(user_id, user, ip, ctx);
            }
            ServerPacket::Settings {
                allow_messages,
                hide_server,
                accept_friend_requests,
                server_chat,
            } => {
                self.handle_settings(user_id, allow_messages, hide_server, accept_friend_requests, server_chat);
            }
            ServerPacket::Friend { action, user } => {
                self.handle_friend(user_id, action, user, ctx);
            }
            ServerPacket::Block { user, blocked } => {
                self.handle_block(user_id, user, blocked, ctx);
            }
            ServerPacket::RequestPunishments { user } => {
                self.handle_request_punishments(user_id, user, ctx);
            }
            ServerPacket::RequestUserCount => {
                self.send_user_count(user_id);
            }
            ServerPacket::Hello { protocol } => {
                self.handle_hello(user_id, protocol);
            }
        }
    }
}
