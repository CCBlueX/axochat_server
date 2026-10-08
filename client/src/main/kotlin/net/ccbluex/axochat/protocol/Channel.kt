package net.ccbluex.axochat.protocol

public sealed interface Channel {

    public data object Global : Channel {
        override fun toString(): String = "global"
    }

    /**
     * Users on the same Minecraft server who turned server chat on.
     */
    public data object Server : Channel {
        override fun toString(): String = "server"
    }

    public data object Party : Channel {
        override fun toString(): String = "party"
    }

    public data class Group(val id: String) : Channel {
        override fun toString(): String = "group/$id"
    }

    /**
     * [user] is an id, or a name when sending.
     */
    public data class User(val user: String) : Channel {
        override fun toString(): String = "user/$user"
    }

    public companion object {
        public fun parse(channel: String): Channel? = when {
            channel == "global" -> Global
            channel == "server" -> Server
            channel == "party" -> Party
            channel.startsWith("group/") -> Group(channel.removePrefix("group/"))
            channel.startsWith("user/") -> User(channel.removePrefix("user/"))
            else -> null
        }
    }
}
