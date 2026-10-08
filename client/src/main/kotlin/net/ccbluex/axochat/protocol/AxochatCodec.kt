package net.ccbluex.axochat.protocol

import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerializationException
import kotlinx.serialization.descriptors.StructureKind
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNamingStrategy
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlin.reflect.KClass

/**
 * Packets travel as `{"m": name, "c": content}`; packets without content leave `c` out.
 */
public object AxochatCodec {

    public val json: Json = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
        namingStrategy = JsonNamingStrategy.SnakeCase
    }

    private val serverbound: Map<KClass<out Serverbound>, KSerializer<out Serverbound>> = listOf(
        entry(Serverbound.Hello.serializer()),
        entry(Serverbound.RequestMojangInfo.serializer()),
        entry(Serverbound.LoginMojang.serializer()),
        entry(Serverbound.LoginAccount.serializer()),
        entry(Serverbound.Message.serializer()),
        entry(Serverbound.PrivateMessage.serializer()),
        entry(Serverbound.RequestUserCount.serializer()),
        entry(Serverbound.Settings.serializer()),
        entry(Serverbound.Friend.serializer()),
        entry(Serverbound.Block.serializer()),
        entry(Serverbound.ChatMessage.serializer()),
        entry(Serverbound.Report.serializer()),
        entry(Serverbound.RequestReports.serializer()),
        entry(Serverbound.ResolveReport.serializer()),
        entry(Serverbound.Punish.serializer()),
        entry(Serverbound.Pardon.serializer()),
        entry(Serverbound.RequestPunishments.serializer()),
        entry(Serverbound.Location.serializer()),
        entry(Serverbound.Sightings.serializer()),
        entry(Serverbound.PartyState.serializer()),
    ).toMap()

    private val clientbound: Map<String, KSerializer<out Clientbound>> = listOf(
        Clientbound.Hello.serializer(),
        Clientbound.MojangInfo.serializer(),
        Clientbound.Message.serializer(),
        Clientbound.PrivateMessage.serializer(),
        Clientbound.UserCount.serializer(),
        Clientbound.Success.serializer(),
        Clientbound.Error.serializer(),
        Clientbound.Punished.serializer(),
        Clientbound.Punishments.serializer(),
        Clientbound.Welcome.serializer(),
        Clientbound.Settings.serializer(),
        Clientbound.Friends.serializer(),
        Clientbound.Presence.serializer(),
        Clientbound.Blocks.serializer(),
        Clientbound.ChatMessage.serializer(),
        Clientbound.Groups.serializer(),
        Clientbound.Reports.serializer(),
        Clientbound.ReportCreated.serializer(),
        Clientbound.Party.serializer(),
        Clientbound.PartyInvite.serializer(),
        Clientbound.PartyWarp.serializer(),
        Clientbound.PartyMemberState.serializer(),
    ).associateBy { it.descriptor.serialName }

    private inline fun <reified T : Serverbound> entry(serializer: KSerializer<T>) =
        T::class to serializer

    @Suppress("UNCHECKED_CAST")
    public fun encode(packet: Serverbound): String {
        val serializer = when (packet) {
            is Serverbound.Group -> Serverbound.Group.serializer()
            is Serverbound.Party -> Serverbound.Party.serializer()
            else -> serverbound[packet::class] ?: throw SerializationException("${packet::class} is not a packet")
        } as KSerializer<Serverbound>

        val envelope = buildJsonObject {
            put("m", JsonPrimitive(serializer.descriptor.serialName))
            if (serializer.descriptor.kind != StructureKind.OBJECT) {
                put("c", json.encodeToJsonElement(serializer, packet))
            }
        }
        return envelope.toString()
    }

    /**
     * A packet this version does not know becomes [Clientbound.Unknown].
     *
     * @throws SerializationException if the frame is no packet, or a known packet is malformed
     */
    public fun decode(text: String): Clientbound {
        val envelope = json.parseToJsonElement(text) as? JsonObject
            ?: throw SerializationException("A packet is a JSON object")
        val name = (envelope["m"] as? JsonPrimitive)?.takeIf { it.isString }?.content
            ?: throw SerializationException("A packet has a name")
        val content = envelope["c"]
        val serializer = clientbound[name] ?: return Clientbound.Unknown(name, content)
        return json.decodeFromJsonElement(serializer, content?.jsonObject ?: JsonObject(emptyMap()))
    }

    public fun decodeOrNull(text: String): Clientbound? = try {
        decode(text)
    } catch (_: SerializationException) {
        null
    } catch (_: IllegalArgumentException) {
        null
    }
}
