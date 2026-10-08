package net.ccbluex.axochat.group

import kotlinx.serialization.Serializable
import net.ccbluex.axochat.user.UserRef

@Serializable
public data class Group(val id: String, val name: String, val role: GroupRole, val members: List<GroupMember> = emptyList())

@Serializable
public data class GroupMember(val user: UserRef, val role: GroupRole, val online: Boolean)

@Serializable
@JvmInline
public value class GroupRole(public val name: String) {
    override fun toString(): String = name

    public companion object {
        public val Owner: GroupRole = GroupRole("owner")
        public val Admin: GroupRole = GroupRole("admin")
        public val Member: GroupRole = GroupRole("member")
        public val Invited: GroupRole = GroupRole("invited")
    }
}
