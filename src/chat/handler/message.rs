use crate::chat::{
    now_ms, Channel, ChatServer, ClientPacket, Connection, Frame, InternalId, Protocol, Recorded, UserId, HISTORY,
};
use crate::entity::punishment::Kind;
use crate::error::*;
use log::*;

use uuid::Uuid;

impl ChatServer {
    pub(super) fn handle_message(&mut self, user_id: InternalId, content: String) {
        self.chat(user_id, Channel::Global, content);
    }

    pub(super) fn handle_private_message(&mut self, user_id: InternalId, receiver: String, content: String) {
        self.chat(user_id, Channel::User(receiver), content);
    }

    pub(super) fn handle_chat_message(&mut self, user_id: InternalId, channel: String, content: String) {
        match Channel::parse(&channel) {
            Some(channel) => self.chat(user_id, channel, content),
            None => {
                self.send(user_id, ClientPacket::error_with(ClientError::UnknownChannel, channel));
            }
        }
    }

    fn chat(&mut self, user_id: InternalId, channel: Channel, content: String) {
        let Some(user) = self.logged_in(user_id) else { return };
        if !self.check_message(user_id, user, &content) {
            return;
        }

        match channel {
            Channel::Global => self.send_global(user, content),
            Channel::User(receiver) => self.send_direct(user_id, user, receiver, content),
            Channel::Group(group) => self.send_group_message(user_id, user, group, content),
            channel => {
                self.send(user_id, ClientPacket::error_with(ClientError::UnknownChannel, channel.to_string()));
            }
        }
    }

    fn send_global(&mut self, user: UserId, content: String) {
        info!("User `{}` has written `{}`.", user, content);
        let message = self.record(Channel::Global, user, &content, None);
        let v1 = ClientPacket::Message {
            author_info: self.identity(user).info(),
            content: content.clone(),
        }
        .encode(Protocol::V1);
        let v2 = self.chat_frame(message, Channel::Global);

        for connection in self.connections.values() {
            match connection.user() {
                Some(recipient) if !self.social.has_blocked(recipient, user) => {
                    send_frame(connection, if connection.protocol == Protocol::V1 { &v1 } else { &v2 });
                }
                _ => {}
            }
        }
    }

    fn send_direct(&mut self, user_id: InternalId, user: UserId, receiver: String, content: String) {
        let sender_protocol = self.connections.get(&user_id).map_or(Protocol::V1, |connection| connection.protocol);
        let target = Uuid::parse_str(&receiver)
            .ok()
            .filter(|id| self.users.contains_key(id))
            .or_else(|| self.find_online(&receiver));
        let Some(target) = target else {
            // v1 never said whether the receiver exists
            if sender_protocol >= Protocol::V2 {
                self.send(user_id, ClientPacket::error_with(ClientError::UnknownUser, receiver));
            }
            return;
        };
        // a block looks like messages not being accepted
        if self.social.has_blocked(target, user) {
            self.send_error(user_id, ClientError::PrivateMessageNotAccepted);
            return;
        }

        let message = self.record(Channel::direct(target), user, &content, Some(vec![user, target]));
        let v1 = ClientPacket::PrivateMessage {
            author_info: self.identity(user).info(),
            content,
        }
        .encode(Protocol::V1);
        let v2 = self.chat_frame(message, Channel::direct(user));

        let mut delivered = false;
        for (_, connection) in self.online_connections(target) {
            if connection.allow_messages {
                delivered |= send_frame(connection, if connection.protocol == Protocol::V1 { &v1 } else { &v2 });
            }
        }
        if !delivered {
            self.send_error(user_id, ClientError::PrivateMessageNotAccepted);
            return;
        }

        info!("User `{}` has written to `{}` privately.", user, target);
        let echo = self.chat_frame(message, Channel::direct(target));
        for (_, connection) in self.online_connections(user) {
            if connection.protocol >= Protocol::V2 {
                send_frame(connection, &echo);
            }
        }
    }

    pub(super) fn record(&mut self, channel: Channel, author: UserId, content: &str, audience: Option<Vec<UserId>>) -> u64 {
        let id = self.next_message_id;
        self.next_message_id += 1;
        if self.history.len() == HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(Recorded {
            id,
            channel,
            author,
            content: content.to_owned(),
            time: now_ms(),
            audience,
        });
        id
    }

    pub(super) fn chat_frame(&self, id: u64, channel: Channel) -> Frame {
        let message = self
            .history
            .iter()
            .rev()
            .find(|message| message.id == id)
            .expect("messages are recorded before they are sent");
        ClientPacket::ChatMessage {
            channel: channel.to_string(),
            id: message.id,
            time: message.time,
            author: self.author(message.author),
            content: message.content.clone(),
        }
        .encode(Protocol::V2)
    }

    pub(super) fn check_message(&mut self, user_id: InternalId, user: UserId, content: &str) -> bool {
        let Some(online) = self.users.get_mut(&user) else { return false };
        if online.rate_limiter.check_new_message(content.to_owned()) {
            info!("User `{}` tried to send message, but was rate limited.", user_id);
            self.send_error(user_id, ClientError::RateLimited);
            return false;
        }

        if let Err(err) = self.validator.validate(content, self.has_perks(user)) {
            info!("User `{}` tried to send invalid message: {}", user_id, err);
            if let Error::AxoChat { source } = err {
                self.send_error(user_id, source);
            }
            return false;
        }

        let Some(connection) = self.connections.get(&user_id) else { return false };
        let muted = self.moderation.find(Kind::Mute, Some(user), Some(connection.ip), now_ms());
        if muted.is_some() && !self.is_staff(user) {
            info!("User `{}` tried to send message while muted", user_id);
            let error = match connection.protocol {
                Protocol::V1 => ClientError::Banned,
                Protocol::V2 => ClientError::Muted,
            };
            self.send_error(user_id, error);
            return false;
        }

        true
    }
}

// try_send would also fail on a full mailbox; only a closed one is a delivery failure.
pub(super) fn send_frame(connection: &Connection, frame: &Frame) -> bool {
    connection.addr.connected() && {
        connection.addr.do_send(frame.clone());
        true
    }
}
