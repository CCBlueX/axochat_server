package net.ccbluex.axochat

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.channelFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.mapNotNull
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.selects.select
import kotlinx.coroutines.withTimeoutOrNull
import net.ccbluex.axochat.protocol.AxochatCodec
import net.ccbluex.axochat.protocol.Clientbound
import net.ccbluex.axochat.protocol.Serverbound
import net.ccbluex.axochat.protocol.SuccessReason
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import java.net.SocketTimeoutException
import java.util.UUID
import kotlin.time.Duration
import kotlin.time.Duration.Companion.seconds

public data class CloseReason(val code: Int, val reason: String, val error: Throwable? = null)

public sealed interface LoginResult {
    public data object Success : LoginResult

    public data class Refused(val error: Clientbound.Error) : LoginResult

    public data object Closed : LoginResult
}

public class AxochatSession internal constructor(
    private val maxFrameSize: Int,
    private val malformed: (String) -> Unit,
) : AutoCloseable {

    private val job = Job()
    private val incoming = MutableSharedFlow<Clientbound>(extraBufferCapacity = 64)
    private val opened = CompletableDeferred<Unit>()
    private lateinit var webSocket: WebSocket

    /**
     * Packets nobody collects are dropped; a slow collector holds back reading from the server.
     */
    public val packets: SharedFlow<Clientbound> = incoming.asSharedFlow()

    /**
     * [packets], completing when the session ends.
     */
    public fun incoming(): Flow<Clientbound> = channelFlow {
        val relay = launch(start = CoroutineStart.UNDISPATCHED) { packets.collect { send(it) } }
        closed.await()
        relay.cancel()
    }

    public val closed: Deferred<CloseReason>
        field = CompletableDeferred<CloseReason>()

    public val isOpen: Boolean get() = !closed.isCompleted

    @Volatile
    public var protocol: Int = 1
        private set

    public val isModern: Boolean get() = protocol >= PROTOCOL_VERSION

    internal val listener = object : WebSocketListener() {
        override fun onOpen(webSocket: WebSocket, response: Response) {
            opened.complete(Unit)
        }

        override fun onMessage(webSocket: WebSocket, text: String) {
            if (text.length > maxFrameSize) {
                webSocket.cancel()
                end(CloseReason(MESSAGE_TOO_BIG, "frame too large"))
                return
            }
            val packet = AxochatCodec.decodeOrNull(text)
            if (packet == null) {
                malformed(text)
                return
            }
            // blocks OkHttp's reader, which keeps the order and the backpressure
            try {
                runBlocking(job) { incoming.emit(packet) }
            } catch (_: CancellationException) {
            }
        }

        override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
            webSocket.close(NORMAL_CLOSURE, null)
            end(CloseReason(code, reason))
        }

        override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
            opened.completeExceptionally(t)
            end(CloseReason(ABNORMAL_CLOSURE, t.message ?: t.javaClass.name, t))
        }
    }

    internal suspend fun open(webSocket: WebSocket, timeout: Duration) {
        this.webSocket = webSocket
        try {
            withTimeoutOrNull(timeout) { opened.await() }
                ?: throw SocketTimeoutException("no handshake within $timeout")
        } catch (e: Throwable) {
            webSocket.cancel()
            throw e
        }
    }

    public fun send(packet: Serverbound): Boolean = isOpen && webSocket.send(AxochatCodec.encode(packet))

    /**
     * `null` if the session ends or [timeout] passes before [answer] maps a packet.
     */
    public suspend fun <T : Any> request(
        packet: Serverbound,
        timeout: Duration = Duration.INFINITE,
        answer: (Clientbound) -> T?,
    ): T? = coroutineScope {
        // listening before sending, so a quick answer cannot slip by
        val reply = async(start = CoroutineStart.UNDISPATCHED) { packets.mapNotNull(answer).first() }
        if (!send(packet)) {
            reply.cancel()
            return@coroutineScope null
        }
        withTimeoutOrNull(timeout) {
            select {
                reply.onAwait { it }
                closed.onAwait { null }
            }
        }.also { reply.cancel() }
    }

    /**
     * Servers without v2 drop the `Hello`, so no answer within [timeout] means v1.
     */
    public suspend fun negotiate(timeout: Duration = 3.seconds): Int {
        val answer = request(Serverbound.Hello(PROTOCOL_VERSION), timeout) { it as? Clientbound.Hello }
        protocol = answer?.protocol?.coerceAtMost(PROTOCOL_VERSION) ?: 1
        return protocol
    }

    public suspend fun loginAccount(token: String, allowMessages: Boolean): LoginResult =
        request(Serverbound.LoginAccount(token, allowMessages)) { it.answerTo(SuccessReason.Login) }
            ?: LoginResult.Closed

    /**
     * [joinServer] joins the session server with the server id, as a Minecraft client does on joining a server.
     */
    public suspend fun loginMojang(
        name: String,
        uuid: UUID,
        allowMessages: Boolean,
        joinServer: suspend (serverId: String) -> Unit,
    ): LoginResult = joinThenLogin(name, uuid, allowMessages, SuccessReason.Login, joinServer)

    /**
     * After an account login, shows others the Minecraft account the client plays on.
     */
    public suspend fun proveMinecraft(
        name: String,
        uuid: UUID,
        joinServer: suspend (serverId: String) -> Unit,
    ): LoginResult = joinThenLogin(name, uuid, false, SuccessReason.Minecraft, joinServer)

    private suspend fun joinThenLogin(
        name: String,
        uuid: UUID,
        allowMessages: Boolean,
        reason: SuccessReason,
        joinServer: suspend (serverId: String) -> Unit,
    ): LoginResult {
        val info = request(Serverbound.RequestMojangInfo) { it as? Clientbound.MojangInfo }
            ?: return LoginResult.Closed
        joinServer(info.sessionHash)
        return request(Serverbound.LoginMojang(name, uuid, allowMessages)) { it.answerTo(reason) }
            ?: LoginResult.Closed
    }

    private fun Clientbound.answerTo(reason: SuccessReason): LoginResult? = when (this) {
        is Clientbound.Success -> LoginResult.Success.takeIf { this.reason == reason }
        is Clientbound.Error -> LoginResult.Refused(this)
        else -> null
    }

    private fun end(reason: CloseReason) {
        if (closed.complete(reason)) {
            job.cancel()
        }
    }

    override fun close() {
        if (!isOpen) {
            return
        }
        if (::webSocket.isInitialized) {
            webSocket.close(NORMAL_CLOSURE, null)
        }
        end(CloseReason(NORMAL_CLOSURE, ""))
    }

    private companion object {
        const val NORMAL_CLOSURE = 1000
        const val ABNORMAL_CLOSURE = 1006
        const val MESSAGE_TOO_BIG = 1009
    }
}
