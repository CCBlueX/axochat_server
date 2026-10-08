@file:UseSerializers(UuidSerializer::class)

package net.ccbluex.axochat.protocol

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.UseSerializers
import kotlinx.serialization.json.JsonClassDiscriminator
import kotlinx.serialization.json.JsonObject
import net.ccbluex.axochat.moderation.PunishmentKind
import net.ccbluex.axochat.party.Position
import net.ccbluex.axochat.party.World
import net.ccbluex.axochat.user.Player
import java.util.UUID

public sealed interface Serverbound {

    @Serializable
    @SerialName("Hello")
    public data class Hello(val protocol: Int) : Serverbound

    @Serializable
    @SerialName("RequestMojangInfo")
    public data object RequestMojangInfo : Serverbound

    /**
     * Also proves the Minecraft session after an account login. Sent after joining the session server
     * with the hash from [Clientbound.MojangInfo].
     */
    @Serializable
    @SerialName("LoginMojang")
    public data class LoginMojang(val name: String, val uuid: UUID, val allowMessages: Boolean) : Serverbound

    @Serializable
    @SerialName("LoginAccount")
    public data class LoginAccount(val token: String, val allowMessages: Boolean) : Serverbound {
        override fun toString(): String = "LoginAccount(allowMessages=$allowMessages)"
    }

    /**
     * Protocol v1; v2 sends [ChatMessage].
     */
    @Serializable
    @SerialName("Message")
    public data class Message(val content: String) : Serverbound

    /**
     * Protocol v1; v2 sends [ChatMessage].
     */
    @Serializable
    @SerialName("PrivateMessage")
    public data class PrivateMessage(val receiver: String, val content: String) : Serverbound

    @Serializable
    @SerialName("RequestUserCount")
    public data object RequestUserCount : Serverbound

    @Serializable
    @SerialName("Settings")
    public data class Settings(
        val allowMessages: Boolean? = null,
        val hideServer: Boolean? = null,
        val acceptFriendRequests: Boolean? = null,
        val serverChat: Boolean? = null,
    ) : Serverbound

    @Serializable
    public enum class FriendAction {
        @SerialName("request") Request,
        @SerialName("accept") Accept,
        @SerialName("decline") Decline,
        @SerialName("remove") Remove,
    }

    @Serializable
    @SerialName("Friend")
    public data class Friend(val action: FriendAction, val user: String) : Serverbound

    @Serializable
    @SerialName("Block")
    public data class Block(val user: String, val blocked: Boolean) : Serverbound

    @Serializable
    @SerialName("ChatMessage")
    public data class ChatMessage(val channel: String, val content: String) : Serverbound {
        public constructor(channel: Channel, content: String) : this(channel.toString(), content)
    }

    @Serializable
    @SerialName("Report")
    public data class Report(val user: String, val message: Long? = null, val reason: String) : Serverbound

    @Serializable
    @SerialName("RequestReports")
    public data object RequestReports : Serverbound

    @Serializable
    @SerialName("ResolveReport")
    public data class ResolveReport(val id: String) : Serverbound

    /**
     * @param duration in seconds, `null` for permanent
     */
    @Serializable
    @SerialName("Punish")
    public data class Punish(
        val user: String? = null,
        val ip: String? = null,
        val kind: PunishmentKind,
        val duration: Long? = null,
        val reason: String,
        val includeIp: Boolean = false,
    ) : Serverbound

    @Serializable
    @SerialName("Pardon")
    public data class Pardon(val user: String? = null, val ip: String? = null) : Serverbound

    @Serializable
    @SerialName("RequestPunishments")
    public data class RequestPunishments(val user: String) : Serverbound

    /**
     * `server` is `null` in singleplayer.
     */
    @Serializable
    @SerialName("Location")
    public data class Location(val server: String? = null, val world: World? = null, val player: Player? = null) : Serverbound

    /**
     * Party members whose player is a loaded entity, or in the tab list.
     */
    @Serializable
    @SerialName("Sightings")
    public data class Sightings(val entities: List<String> = emptyList(), val tab: List<String> = emptyList()) : Serverbound

    @Serializable
    @SerialName("PartyState")
    public data class PartyState(
        val position: Position? = null,
        val status: JsonObject? = null,
        val inventory: JsonObject? = null,
    ) : Serverbound

    @Serializable
    @SerialName("Group")
    @JsonClassDiscriminator("action")
    public sealed interface Group : Serverbound {
        @Serializable @SerialName("create")
        public data class Create(val name: String) : Group

        @Serializable @SerialName("rename")
        public data class Rename(val group: String, val name: String) : Group

        @Serializable @SerialName("invite")
        public data class Invite(val group: String, val user: String) : Group

        @Serializable @SerialName("accept")
        public data class Accept(val group: String) : Group

        @Serializable @SerialName("decline")
        public data class Decline(val group: String) : Group

        @Serializable @SerialName("leave")
        public data class Leave(val group: String) : Group

        @Serializable @SerialName("kick")
        public data class Kick(val group: String, val user: String) : Group

        @Serializable @SerialName("promote")
        public data class Promote(val group: String, val user: String, val admin: Boolean) : Group

        @Serializable @SerialName("delete")
        public data class Delete(val group: String) : Group
    }

    @Serializable
    @SerialName("Party")
    @JsonClassDiscriminator("action")
    public sealed interface Party : Serverbound {
        @Serializable @SerialName("invite")
        public data class Invite(val user: String) : Party

        @Serializable @SerialName("accept")
        public data class Accept(val party: String) : Party

        @Serializable @SerialName("decline")
        public data class Decline(val party: String) : Party

        @Serializable @SerialName("leave")
        public data object Leave : Party

        @Serializable @SerialName("kick")
        public data class Kick(val user: String) : Party

        @Serializable @SerialName("promote")
        public data class Promote(val user: String, val admin: Boolean) : Party

        @Serializable @SerialName("transfer")
        public data class Transfer(val user: String) : Party

        @Serializable @SerialName("lock")
        public data class Lock(val locked: Boolean) : Party

        @Serializable @SerialName("mute")
        public data class Mute(val user: String, val muted: Boolean) : Party

        @Serializable @SerialName("pvp")
        public data class Pvp(val enabled: Boolean) : Party

        @Serializable @SerialName("warp")
        public data object Warp : Party

        @Serializable @SerialName("disband")
        public data object Disband : Party
    }
}
