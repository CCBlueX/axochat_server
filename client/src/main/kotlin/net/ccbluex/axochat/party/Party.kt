package net.ccbluex.axochat.party

import kotlinx.serialization.Serializable
import net.ccbluex.axochat.user.Player
import net.ccbluex.axochat.user.UserRef

@Serializable
public data class PartyInfo(
    val id: String,
    val leader: String,
    val locked: Boolean,
    val pvp: Boolean,
    val members: List<PartyMember> = emptyList(),
)

@Serializable
public data class PartyMember(
    val user: UserRef,
    val role: PartyRole,
    val online: Boolean,
    val muted: Boolean,
    val relation: Relation,
    val player: Player? = null,
    val server: String? = null,
)

@Serializable
@JvmInline
public value class PartyRole(public val name: String) {
    override fun toString(): String = name

    public companion object {
        public val Leader: PartyRole = PartyRole("leader")
        public val Admin: PartyRole = PartyRole("admin")
        public val Member: PartyRole = PartyRole("member")
    }
}

/**
 * Relative to the receiver.
 */
@Serializable
@JvmInline
public value class Relation(public val name: String) {
    override fun toString(): String = name

    public companion object {
        public val Self: Relation = Relation("self")
        public val Nearby: Relation = Relation("nearby")
        public val World: Relation = Relation("world")
        public val Instance: Relation = Relation("instance")
        public val Server: Relation = Relation("server")
        public val Elsewhere: Relation = Relation("elsewhere")
        public val Offline: Relation = Relation("offline")
    }
}

@Serializable
public data class Position(
    val x: Double,
    val y: Double,
    val z: Double,
    val yaw: Float,
    val pitch: Float,
    val dimension: String? = null,
)

/**
 * @param seed the hashed seed, a string because JavaScript clients cannot hold it as a number
 * @param age game time in ticks, `null` while it does not advance
 */
@Serializable
public data class World(val dimension: String, val seed: String, val age: Long? = null) {
    public constructor(dimension: String, seed: Long, age: Long?) : this(dimension, seed.toString(), age)
}
