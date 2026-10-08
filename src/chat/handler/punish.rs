use crate::chat::{
    new_id, now_ms, ChatServer, ClientPacket, Identity, InternalId, Login, PunishmentView, Resolved, SuccessReason,
    UserId, UserRef,
};
use crate::entity::punishment::Kind;
use crate::error::ClientError;
use crate::ip::Cidr;
use crate::moderation::Punishment;
use crate::store::{MinecraftTargets, Write};
use log::*;

use actix::*;
use uuid::Uuid;

const MAX_REASON: usize = 256;

impl ChatServer {
    fn staff(&self, user_id: InternalId) -> Option<UserId> {
        let user = self.logged_in(user_id)?;
        if !self.is_staff(user) {
            info!("`{}` tried to moderate without permission", user_id);
            self.send_error(user_id, ClientError::NotPermitted);
            return None;
        }
        Some(user)
    }

    /// v1 bans are permanent mutes of everyone behind a Minecraft UUID.
    pub(super) fn ban_user(&mut self, user_id: InternalId, uuid: Uuid, ctx: &mut Context<Self>) {
        let Some(staff) = self.staff(user_id) else { return };
        if uuid.is_nil() {
            self.send_error(user_id, ClientError::InvalidId);
            return;
        }

        let store = self.store.clone();
        let create = Some((new_id(&mut self.rng), now_ms()));
        ctx.spawn(
            async move { store.send(MinecraftTargets { uuid, create }).await }
                .into_actor(self)
                .map(move |result, actor, _ctx| {
                    let Ok(Ok(targets)) = result else {
                        error!("Could not look up users of `{}`", uuid);
                        actor.send_error(user_id, ClientError::Internal);
                        return;
                    };
                    if targets.iter().any(|target| actor.is_staff(target.id)) {
                        actor.send_error(user_id, ClientError::NotPermitted);
                        return;
                    }

                    for target in &targets {
                        let identity = Identity::of(target);
                        actor.directory.entry(identity.id).or_insert(identity);
                        let punishment = Punishment {
                            id: new_id(&mut actor.rng),
                            kind: Kind::Mute,
                            user: Some(target.id),
                            ip: None,
                            reason: String::new(),
                            issued_by: Some(staff),
                            created_at: now_ms(),
                            expires_at: None,
                        };
                        actor.punish(punishment);
                    }
                    info!("User `{}` banned.", uuid);
                    actor.send(user_id, ClientPacket::Success { reason: SuccessReason::Ban });
                }),
        );
    }

    pub(super) fn unban_user(&mut self, user_id: InternalId, uuid: Uuid, ctx: &mut Context<Self>) {
        let Some(staff) = self.staff(user_id) else { return };

        let store = self.store.clone();
        ctx.spawn(
            async move { store.send(MinecraftTargets { uuid, create: None }).await }
                .into_actor(self)
                .map(move |result, actor, _ctx| {
                    let Ok(Ok(targets)) = result else {
                        error!("Could not look up users of `{}`", uuid);
                        actor.send_error(user_id, ClientError::Internal);
                        return;
                    };

                    let mut pardoned = false;
                    for target in targets {
                        pardoned |= actor.pardon(staff, Some(target.id), None);
                    }
                    if pardoned {
                        info!("User `{}` unbanned.", uuid);
                        actor.send(user_id, ClientPacket::Success { reason: SuccessReason::Unban });
                    } else {
                        actor.send_error(user_id, ClientError::NotBanned);
                    }
                }),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_punish(
        &mut self,
        user_id: InternalId,
        user: Option<String>,
        ip: Option<String>,
        kind: Kind,
        duration: Option<u64>,
        reason: String,
        include_ip: bool,
        ctx: &mut Context<Self>,
    ) {
        let Some(staff) = self.staff(user_id) else { return };
        let reason: String = reason.chars().take(MAX_REASON).collect();
        let created_at = now_ms();
        let expires_at = duration.map(|seconds| created_at.saturating_add((seconds as i64).saturating_mul(1000)));

        match (user, ip) {
            (Some(query), _) => self.resolve_user(ctx, query.clone(), move |actor, _ctx, resolved| {
                let Some(Resolved { identity, last_ip, .. }) = resolved else {
                    actor.send(user_id, ClientPacket::error_with(ClientError::UnknownUser, query));
                    return;
                };
                if actor.is_staff(identity.id) {
                    actor.send_error(user_id, ClientError::NotPermitted);
                    return;
                }

                let punishment = Punishment {
                    id: new_id(&mut actor.rng),
                    kind,
                    user: Some(identity.id),
                    ip: last_ip.filter(|_| include_ip).map(Cidr::ban_range),
                    reason,
                    issued_by: Some(staff),
                    created_at,
                    expires_at,
                };
                info!("`{}` punished `{}` ({:?}).", staff, identity.name, kind);
                actor.punish(punishment);
                actor.send(user_id, ClientPacket::Success { reason: SuccessReason::Punish });
            }),
            (None, Some(ip)) => {
                let Ok(ip) = ip.parse::<Cidr>() else {
                    self.send(user_id, ClientPacket::error_with(ClientError::InvalidId, ip));
                    return;
                };
                let punishment = Punishment {
                    id: new_id(&mut self.rng),
                    kind,
                    user: None,
                    ip: Some(ip),
                    reason,
                    issued_by: Some(staff),
                    created_at,
                    expires_at,
                };
                info!("`{}` punished {} ({:?}).", staff, ip, kind);
                self.punish(punishment);
                self.send(user_id, ClientPacket::Success { reason: SuccessReason::Punish });
            }
            (None, None) => self.send_error(user_id, ClientError::UnknownUser),
        }
    }

    pub(super) fn handle_pardon(&mut self, user_id: InternalId, user: Option<String>, ip: Option<String>, ctx: &mut Context<Self>) {
        let Some(staff) = self.staff(user_id) else { return };

        let answer = move |actor: &mut ChatServer, pardoned: bool| {
            if pardoned {
                actor.send(user_id, ClientPacket::Success { reason: SuccessReason::Pardon });
            } else {
                actor.send_error(user_id, ClientError::NotBanned);
            }
        };
        match (user, ip) {
            (Some(query), _) => self.resolve_user(ctx, query.clone(), move |actor, _ctx, resolved| {
                let Some(resolved) = resolved else {
                    actor.send(user_id, ClientPacket::error_with(ClientError::UnknownUser, query));
                    return;
                };
                let pardoned = actor.pardon(staff, Some(resolved.identity.id), None);
                answer(actor, pardoned);
            }),
            (None, Some(ip)) => {
                let Ok(ip) = ip.parse::<Cidr>() else {
                    self.send(user_id, ClientPacket::error_with(ClientError::InvalidId, ip));
                    return;
                };
                let pardoned = self.pardon(staff, None, Some(ip));
                answer(self, pardoned);
            }
            (None, None) => self.send_error(user_id, ClientError::UnknownUser),
        }
    }

    pub(super) fn handle_request_punishments(&mut self, user_id: InternalId, user: String, ctx: &mut Context<Self>) {
        if self.staff(user_id).is_none() {
            return;
        }

        self.resolve_user(ctx, user.clone(), move |actor, _ctx, resolved| {
            let Some(resolved) = resolved else {
                actor.send(user_id, ClientPacket::error_with(ClientError::UnknownUser, user));
                return;
            };
            let punishments = actor
                .moderation
                .of_user(resolved.identity.id, now_ms())
                .into_iter()
                .map(|punishment| PunishmentView {
                    id: punishment.id,
                    kind: punishment.kind,
                    ip: punishment.ip.map(|ip| ip.to_string()),
                    reason: punishment.reason.clone(),
                    issued_by: punishment
                        .issued_by
                        .and_then(|issuer| actor.directory.get(&issuer))
                        .map(UserRef::from),
                    created: punishment.created_at,
                    expires: punishment.expires_at,
                })
                .collect();
            actor.send_v2(
                user_id,
                ClientPacket::Punishments {
                    user: UserRef::from(&resolved.identity),
                    punishments,
                },
            );
        });
    }

    fn punish(&mut self, punishment: Punishment) {
        self.persist(vec![Write::Punish(punishment.to_model())]);

        let covered: Vec<InternalId> = self
            .connections
            .iter()
            .filter_map(|(id, connection)| match connection.login {
                Login::User(user) if punishment.covers(Some(user), Some(connection.ip)) && !self.is_staff(user) => {
                    Some(*id)
                }
                _ => None,
            })
            .collect();
        for id in covered {
            self.send_v2(
                id,
                ClientPacket::Punished {
                    kind: punishment.kind,
                    reason: punishment.reason.clone(),
                    expires: punishment.expires_at,
                },
            );
            if punishment.kind == Kind::Ban {
                self.send_error(id, ClientError::Banned);
                self.logout(id);
            }
        }

        self.moderation.add(punishment);
    }

    fn pardon(&mut self, staff: UserId, user: Option<UserId>, ip: Option<Cidr>) -> bool {
        let ids = self.moderation.revoke(user, ip);
        if ids.is_empty() {
            return false;
        }
        self.persist(vec![Write::Revoke {
            ids,
            by: Some(staff),
            at: now_ms(),
        }]);
        true
    }
}
