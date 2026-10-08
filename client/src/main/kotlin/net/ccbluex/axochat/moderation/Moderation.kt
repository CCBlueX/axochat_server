package net.ccbluex.axochat.moderation

import kotlinx.serialization.Serializable
import net.ccbluex.axochat.user.UserRef

@Serializable
public data class Report(
    val id: String,
    val reporter: UserRef,
    val target: UserRef,
    val channel: String? = null,
    val message: Long? = null,
    val content: String? = null,
    val reason: String,
    val time: Long,
)

@Serializable
public data class Punishment(
    val id: String,
    val kind: PunishmentKind,
    val ip: String? = null,
    val reason: String,
    val issuedBy: UserRef? = null,
    val created: Long,
    val expires: Long? = null,
)

@Serializable
@JvmInline
public value class PunishmentKind(public val name: String) {
    override fun toString(): String = name

    public companion object {
        public val Mute: PunishmentKind = PunishmentKind("mute")
        public val Ban: PunishmentKind = PunishmentKind("ban")
    }
}
