@file:UseSerializers(UuidSerializer::class)

package net.ccbluex.axochat.user

import kotlinx.serialization.Serializable
import kotlinx.serialization.UseSerializers
import net.ccbluex.axochat.protocol.UuidSerializer
import java.util.UUID

@Serializable
public data class Player(val uuid: UUID, val name: String)

/**
 * @param uuid the account shown as their head, nil if unknown
 * @param minecraft the Minecraft account an online LiquidBounce Account proved it plays on
 */
@Serializable
public data class UserRef(
    val id: String,
    val kind: UserKind,
    val name: String,
    val uuid: UUID,
    val minecraft: Player? = null,
) {
    val isAccount: Boolean get() = kind == UserKind.Account
}

/**
 * [Mojang] is a Minecraft account without a LiquidBounce Account.
 */
@Serializable
@JvmInline
public value class UserKind(public val name: String) {
    override fun toString(): String = name

    public companion object {
        public val Account: UserKind = UserKind("account")
        public val Mojang: UserKind = UserKind("mojang")
    }
}

@Serializable
public data class Role(val id: String, val name: String, val staff: Boolean)

/**
 * @param roles staff roles first
 */
@Serializable
public data class Author(
    val id: String,
    val kind: UserKind,
    val name: String,
    val uuid: UUID,
    val minecraft: Player? = null,
    val roles: List<Role> = emptyList(),
    val highlight: Boolean = false,
) {
    val isAccount: Boolean get() = kind == UserKind.Account

    public fun toUserRef(): UserRef = UserRef(id, kind, name, uuid, minecraft)
}

/**
 * Protocol v1.
 */
@Serializable
public data class LegacyUser(val name: String, val uuid: UUID)

@Serializable
public data class Friend(val user: UserRef, val since: Long, val online: Boolean, val server: String? = null)
