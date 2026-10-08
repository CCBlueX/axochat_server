package net.ccbluex.axochat

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import okhttp3.OkHttpClient
import okhttp3.Request
import java.net.URI
import kotlin.random.Random
import kotlin.time.Duration
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Duration.Companion.seconds
import kotlin.time.TimeSource

public const val PROTOCOL_VERSION: Int = 2

/**
 * @param maxFrameSize in characters
 * @param malformed gets frames this version cannot decode
 */
public class AxochatClient(
    public val uri: URI,
    private val http: OkHttpClient = OkHttpClient(),
    private val maxFrameSize: Int = 256 * 1024,
    private val malformed: (String) -> Unit = {},
) {

    public suspend fun connect(timeout: Duration = 10.seconds): AxochatSession {
        val session = AxochatSession(maxFrameSize, malformed)
        val request = Request.Builder().url(uri.toString()).build()
        session.open(http.newWebSocket(request, session.listener), timeout)
        return session
    }

    /**
     * Runs [block] on every new session and reconnects after [backoff] once the session ends.
     * Failures to connect or in [block] go to [failed].
     */
    public suspend fun keepConnected(
        backoff: Backoff = Backoff(),
        failed: (Exception) -> Unit = {},
        block: suspend AxochatSession.() -> Unit,
    ): Nothing {
        var attempt = 0
        while (true) {
            val started = TimeSource.Monotonic.markNow()
            try {
                val session = connect()

                session.use { session ->
                    session.block()
                    session.closed.await()
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                failed(e)
            }
            if (started.elapsedNow() >= backoff.reset) {
                attempt = 0
            }
            delay(backoff.delay(attempt++))
        }
    }
}

/**
 * [initial], doubling up to [max], each varied by [jitter] either way.
 * A session that lasted [reset] starts over at [initial].
 */
public data class Backoff(
    val initial: Duration = 2.seconds,
    val max: Duration = 2.minutes,
    val reset: Duration = 1.minutes,
    val jitter: Double = 0.25,
) {
    public fun delay(attempt: Int): Duration {
        val base = (initial * (1 shl attempt.coerceIn(0, 16))).coerceAtMost(max)
        return base * Random.nextDouble(1 - jitter, 1 + jitter)
    }
}
