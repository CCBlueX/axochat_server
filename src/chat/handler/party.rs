use super::message::send_frame;
use crate::chat::party::{Change, PartyId};
use crate::chat::world::{contradicts_seed, relation, server_key, Location, Player, Relation};
use crate::chat::{
    new_id, now_ms, Channel, ChatServer, ClientPacket, Connection, Frame, InternalId, MemberState, PartyAction,
    PartyMemberView, PartyView, Position, Protocol, Scope, UserId, World,
};
use crate::error::ClientError;
use log::*;

use actix::*;
use std::collections::HashSet;
use uuid::Uuid;

/// Status and inventory together, serialized.
const MAX_STATE: usize = 16 * 1024;
/// 2^53 ticks would overflow the epoch in milliseconds.
const MAX_AGE: i64 = 1 << 47;

impl ChatServer {
    pub(super) fn handle_party(&mut self, user_id: InternalId, action: PartyAction, ctx: &mut Context<Self>) {
        let Some(user) = self.account_user(user_id) else { return };
        let now = now_ms();
        match action {
            PartyAction::Invite { user: query } => self.resolve_user(ctx, query, Scope::Accounts, move |actor, _ctx, resolved| {
                // an invite to a name nobody has, to someone offline or to someone who blocked the inviter vanishes
                let target = resolved
                    .map(|resolved| resolved.identity.id)
                    .filter(|target| actor.users.contains_key(target) && !actor.social.has_blocked(*target, user));
                let new_party = new_id(&mut actor.rng);
                match actor.parties.invite(user, target, new_party, now) {
                    Ok(party) => {
                        if let Some(target) = target {
                            let invite = ClientPacket::PartyInvite {
                                party,
                                from: actor.user_ref(user),
                                expires: now + crate::chat::party::INVITE_TIME,
                            };
                            actor.send_user_v2(target, invite);
                        }
                        actor.refresh_party(user);
                    }
                    Err(error) => actor.send_error(user_id, error),
                }
            }),
            PartyAction::Accept { party } => match self.parties.accept(user, party, now) {
                Ok(change) => {
                    self.apply_change(change);
                    self.send_member_states(user);
                }
                Err(error) => self.send_error(user_id, error),
            },
            PartyAction::Decline { party } => {
                if let Err(error) = self.parties.decline(user, party) {
                    self.send_error(user_id, error);
                }
            }
            PartyAction::Leave => {
                let result = self.parties.leave(user);
                self.party_result(user_id, result);
            }
            PartyAction::Disband => {
                let result = self.parties.disband(user);
                self.party_result(user_id, result);
            }
            PartyAction::Kick { user: query } => self.with_member(user_id, user, query, move |actor, target| {
                let result = actor.parties.kick(user, target);
                actor.party_result(user_id, result);
            }),
            PartyAction::Promote { user: query, admin } => self.with_member(user_id, user, query, move |actor, target| {
                let result = actor.parties.promote(user, target, admin);
                actor.party_updated(user_id, result);
            }),
            PartyAction::Transfer { user: query } => self.with_member(user_id, user, query, move |actor, target| {
                let result = actor.parties.transfer(user, target);
                actor.party_updated(user_id, result);
            }),
            PartyAction::Mute { user: query, muted } => self.with_member(user_id, user, query, move |actor, target| {
                let result = actor.parties.mute(user, target, muted);
                actor.party_updated(user_id, result);
            }),
            PartyAction::Lock { locked } => {
                let result = self.parties.lock(user, locked);
                self.party_updated(user_id, result);
            }
            PartyAction::Pvp { enabled } => {
                let result = self.parties.pvp(user, enabled);
                self.party_updated(user_id, result);
            }
            PartyAction::Warp => self.warp(user_id, user),
        }
    }

    fn with_member<F>(&mut self, user_id: InternalId, user: UserId, query: String, then: F)
    where
        F: FnOnce(&mut ChatServer, UserId),
    {
        let Some(party) = self.parties.of(user) else {
            self.send_error(user_id, ClientError::NotInParty);
            return;
        };
        let member = party.users().find(|member| {
            member.to_string() == query
                || self.directory.get(member).is_some_and(|identity| identity.name.eq_ignore_ascii_case(&query))
        });
        match member {
            Some(member) => then(self, member),
            None => {
                self.send(user_id, ClientPacket::error_with(ClientError::UnknownUser, query));
            }
        }
    }

    fn party_result(&mut self, user_id: InternalId, result: Result<Change, ClientError>) {
        match result {
            Ok(change) => self.apply_change(change),
            Err(error) => self.send_error(user_id, error),
        }
    }

    fn party_updated(&mut self, user_id: InternalId, result: Result<PartyId, ClientError>) {
        match result {
            Ok(party) => self.refresh_members(party),
            Err(error) => self.send_error(user_id, error),
        }
    }

    fn apply_change(&mut self, change: Change) {
        for removed in &change.removed {
            self.member_states.remove(removed);
            self.send_party(*removed);
        }
        if let Some(party) = change.party.filter(|_| !change.disbanded) {
            self.refresh_members(party);
        }
    }

    fn warp(&mut self, user_id: InternalId, user: UserId) {
        let Some(party) = self.parties.of(user) else {
            self.send_error(user_id, ClientError::NotInParty);
            return;
        };
        if party.leader() != user {
            self.send_error(user_id, ClientError::NotPermitted);
            return;
        }
        // private addresses never leave the server
        let Some(server) = self.game_location(user).and_then(|location| location.address.clone()) else {
            self.send(user_id, ClientPacket::error_with(ClientError::NotSupported, "server"));
            return;
        };
        let packet = ClientPacket::PartyWarp {
            from: self.user_ref(user),
            server,
        };
        let members: Vec<UserId> = party.users().filter(|member| *member != user).collect();
        for member in members {
            self.send_user_v2(member, packet.clone());
        }
    }

    pub(super) fn send_user_v2(&self, user: UserId, packet: ClientPacket) {
        for (id, _) in self.online_connections(user) {
            self.send_v2(id, packet.clone());
        }
    }

    fn party_view(&self, viewer: UserId) -> Option<PartyView> {
        let party = self.parties.of(viewer)?;
        let viewer_location = self.game_location(viewer);
        let members = party
            .members
            .iter()
            .filter_map(|member| {
                let location = self.game_location(member.user);
                Some(PartyMemberView {
                    user: self.known_ref(member.user)?,
                    role: member.role,
                    online: self.users.contains_key(&member.user),
                    muted: member.muted,
                    relation: self.relation(viewer, viewer_location, member.user, location),
                    player: location.and_then(|location| location.player.clone()),
                    server: location.and_then(|location| location.address.clone()),
                })
            })
            .collect();
        Some(PartyView {
            id: party.id,
            leader: party.leader(),
            locked: party.locked,
            pvp: party.pvp,
            members,
        })
    }

    fn relation(&self, a: UserId, a_location: Option<&Location>, b: UserId, b_location: Option<&Location>) -> Relation {
        relation(a_location, a, b_location, b, &self.unreliable_seeds)
    }

    /// Sends a user their party, if it changed since the last time.
    pub(in crate::chat) fn send_party(&mut self, viewer: UserId) {
        if !self.users.contains_key(&viewer) {
            self.party_snapshots.remove(&viewer);
            return;
        }
        let packet = ClientPacket::Party {
            party: self.party_view(viewer),
        };
        let frame = packet.encode(Protocol::V2);
        if self.party_snapshots.get(&viewer).is_some_and(|last| *last == *frame.0) {
            return;
        }
        self.party_snapshots.insert(viewer, frame.0.to_string());
        self.send_frame_v2(viewer, &frame);
    }

    fn send_frame_v2(&self, user: UserId, frame: &Frame) {
        for (_, connection) in self.online_connections(user) {
            if connection.protocol >= Protocol::V2 {
                send_frame(connection, frame);
            }
        }
    }

    pub(in crate::chat) fn refresh_party(&mut self, user: UserId) {
        match self.parties.of(user).map(|party| party.id) {
            Some(party) => self.refresh_members(party),
            None => self.send_party(user),
        }
    }

    fn refresh_members(&mut self, party: PartyId) {
        let members: Vec<UserId> = self.parties.get(party).map(|party| party.users().collect()).unwrap_or_default();
        for member in members {
            self.send_party(member);
        }
    }

    pub(in crate::chat) fn party_tick(&mut self) {
        for change in self.parties.expire(now_ms()) {
            self.apply_change(change);
        }
        let members: Vec<UserId> = self.parties.all().flat_map(|party| party.users()).collect();
        for member in members {
            self.send_party(member);
        }
    }

    /// The party as part of the login snapshots, before `Success`.
    pub(in crate::chat) fn party_welcome(&mut self, user: UserId) {
        self.party_snapshots.remove(&user);
        self.parties.set_online(user, true, now_ms());
        self.send_party(user);
    }

    pub(in crate::chat) fn party_login(&mut self, user: UserId) {
        self.refresh_party(user);
        self.send_member_states(user);
    }

    pub(super) fn handle_location(&mut self, user_id: InternalId, server: Option<String>, world: Option<World>, player: Option<Player>) {
        let Some(user) = self.logged_in(user_id) else { return };
        let now = now_ms();
        let Some(connection) = self.connections.get_mut(&user_id) else { return };
        if !connection.limits.location.allow() {
            return;
        }

        let (address, key) = match server.as_deref().and_then(|server| server_key(server, connection.ip)) {
            Some((address, key)) => (address, Some(key)),
            None => (None, None),
        };
        let previous = connection.location.take();
        let (dimension, seed, epoch) = match world {
            Some(world) => (
                Some(world.dimension.chars().take(64).collect()),
                world.seed,
                world.age.filter(|age| (0..MAX_AGE).contains(age)).map(|age| now - age * 50),
            ),
            None => (None, 0, None),
        };
        // sightings belong to a world; a new one starts without
        let same_world = previous
            .as_ref()
            .is_some_and(|previous| previous.key == key && previous.dimension == dimension && previous.seed == seed);
        let (entities, tab) = match previous {
            Some(previous) if same_world => (previous.entities, previous.tab),
            _ => (HashSet::new(), HashSet::new()),
        };
        let address_before = self.visible_server(user);
        let connection = self.connections.get_mut(&user_id).expect("checked above");
        connection.location = Some(Location {
            key,
            address,
            dimension,
            seed,
            epoch,
            player: player.map(|player| Player {
                uuid: player.uuid,
                name: player.name.chars().take(16).collect(),
            }),
            entities,
            tab,
        });
        if let Some(online) = self.users.get_mut(&user) {
            online.game = Some(user_id);
        }

        if self.visible_server(user) != address_before {
            self.send_presence(user);
        }
        self.refresh_party(user);
    }

    pub(super) fn handle_sightings(&mut self, user_id: InternalId, entities: Vec<Uuid>, tab: Vec<Uuid>) {
        let Some(user) = self.logged_in(user_id) else { return };
        let members: HashSet<UserId> = self.parties.of(user).map(|party| party.users().collect()).unwrap_or_default();
        let Some(connection) = self.connections.get_mut(&user_id) else { return };
        if !connection.limits.sightings.allow() {
            return;
        }
        let Some(location) = connection.location.as_mut() else { return };
        location.entities = entities.into_iter().filter(|id| members.contains(id)).collect();
        location.tab = tab.into_iter().filter(|id| members.contains(id)).collect();

        let mut contradicted = Vec::new();
        if let Some(location) = self.game_location(user) {
            for member in &members {
                if let Some(other) = self.game_location(*member) {
                    if contradicts_seed(location, user, other, *member) {
                        contradicted.extend(location.key.clone());
                    }
                }
            }
        }
        for key in contradicted {
            info!("Hashed seeds on `{}` are unreliable.", key);
            self.unreliable_seeds.insert(key);
        }
        self.refresh_party(user);
    }

    pub(super) fn handle_party_state(
        &mut self,
        user_id: InternalId,
        position: Option<Position>,
        status: Option<serde_json::Value>,
        inventory: Option<serde_json::Value>,
    ) {
        let Some(user) = self.logged_in(user_id) else { return };
        let Some(party) = self.parties.of(user) else { return };
        let members: Vec<UserId> = party.users().filter(|member| *member != user).collect();

        let size = [&status, &inventory]
            .into_iter()
            .flatten()
            .map(|value| value.to_string().len())
            .sum::<usize>();
        if size > MAX_STATE {
            self.send_error(user_id, ClientError::TooLarge);
            return;
        }

        let Some(connection) = self.connections.get_mut(&user_id) else { return };
        let limits = &mut connection.limits;
        let position = position.filter(|position| position.is_valid() && limits.position.allow());
        let status = status.filter(|status| status.is_object() && limits.status.allow());
        let inventory = inventory.filter(|inventory| inventory.is_object() && limits.inventory.allow());
        if position.is_none() && status.is_none() && inventory.is_none() {
            return;
        }

        let state = self.member_states.entry(user).or_default();
        if position.is_some() {
            state.position = position.clone();
        }
        if status.is_some() {
            state.status = status.clone();
        }
        if inventory.is_some() {
            state.inventory = inventory.clone();
        }

        let location = self.game_location(user);
        for member in members {
            let shares = self.relation(member, self.game_location(member), user, location).shares_position();
            let position = position.clone().filter(|_| shares);
            if position.is_none() && status.is_none() && inventory.is_none() {
                continue;
            }
            let frame = ClientPacket::PartyMemberState {
                member: user,
                position,
                status: status.clone(),
                inventory: inventory.clone(),
            }
            .encode(Protocol::V2);
            for (_, connection) in self.online_connections(member) {
                if connection.protocol >= Protocol::V2 {
                    send_droppable(connection, &frame);
                }
            }
        }
    }

    fn send_member_states(&self, user: UserId) {
        let Some(party) = self.parties.of(user) else { return };
        let location = self.game_location(user);
        for member in party.users().filter(|member| *member != user) {
            let Some(MemberState { position, status, inventory }) = self.member_states.get(&member) else { continue };
            let shares = self.relation(user, location, member, self.game_location(member)).shares_position();
            let packet = ClientPacket::PartyMemberState {
                member,
                position: position.clone().filter(|_| shares),
                status: status.clone(),
                inventory: inventory.clone(),
            };
            self.send_user_v2(user, packet);
        }
    }

    pub(super) fn send_party_message(&mut self, user_id: InternalId, user: UserId, content: String) {
        let Some(party) = self.parties.of(user) else {
            self.send_error(user_id, ClientError::NotInParty);
            return;
        };
        if party.member(user).is_some_and(|member| member.muted) {
            self.send_error(user_id, ClientError::Muted);
            return;
        }
        let members: Vec<UserId> = party.users().collect();
        let message = self.record(Channel::Party, user, &content, Some(members.clone()));
        let frame = self.chat_frame(message, Channel::Party);
        for member in members {
            if !self.social.has_blocked(member, user) {
                self.send_frame_v2(member, &frame);
            }
        }
    }

    pub(super) fn send_server_message(&mut self, user_id: InternalId, user: UserId, content: String) {
        let key = self.connections.get(&user_id).and_then(|connection| {
            connection
                .location
                .as_ref()
                .filter(|_| connection.server_chat)
                .and_then(|location| location.key.clone())
        });
        let Some(key) = key else {
            self.send(user_id, ClientPacket::error_with(ClientError::UnknownChannel, "server"));
            return;
        };

        let message = self.record(Channel::Server, user, &content, None);
        let frame = self.chat_frame(message, Channel::Server);
        for connection in self.connections.values() {
            let listens = connection.server_chat
                && connection.protocol >= Protocol::V2
                && connection.location.as_ref().is_some_and(|location| location.key.as_ref() == Some(&key));
            if listens && connection.user().is_some_and(|recipient| !self.social.has_blocked(recipient, user)) {
                send_frame(connection, &frame);
            }
        }
    }
}

/// Live state is worthless once late, so a full mailbox drops it.
fn send_droppable(connection: &Connection, frame: &Frame) {
    let _ = connection.addr.try_send(frame.clone());
}
