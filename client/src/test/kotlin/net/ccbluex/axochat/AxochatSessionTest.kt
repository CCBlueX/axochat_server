package net.ccbluex.axochat

import kotlinx.coroutines.async
import kotlinx.coroutines.flow.filterIsInstance
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import net.ccbluex.axochat.protocol.Channel
import net.ccbluex.axochat.protocol.Clientbound
import net.ccbluex.axochat.protocol.ErrorCode
import net.ccbluex.axochat.protocol.Serverbound
import java.net.URI
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.time.Duration.Companion.seconds

/**
 * Against a running server with the test Service API mock, which takes `tok:<sub>:<nickname>:<roles>` tokens;
 * skipped without `AXOCHAT_URL`.
 */
class AxochatSessionTest {

    private val url = System.getenv("AXOCHAT_URL")

    @Test
    fun `negotiates, logs in and chats`() = runBlocking {
        if (url == null) {
            println("AXOCHAT_URL is not set, skipping")
            return@runBlocking
        }
        val run = System.nanoTime().toString(36)
        val session = AxochatClient(URI(url)).connect()
        session.use {
            assertEquals(PROTOCOL_VERSION, it.negotiate())

            val welcome = async { it.packets.filterIsInstance<Clientbound.Welcome>().first() }
            assertEquals(LoginResult.Success, it.loginAccount("tok:kotlin-$run:Kotlin$run:", allowMessages = true))
            assertEquals("Kotlin$run", welcome.await().user.name)

            val echo = it.request(Serverbound.ChatMessage(Channel.Global, "hello from kotlin")) { packet ->
                packet as? Clientbound.ChatMessage
            }
            assertEquals("hello from kotlin", echo?.content)
            assertEquals(Channel.Global, echo?.target)

            val refused = it.loginAccount("tok:again:Again:", allowMessages = true)
            assertEquals(ErrorCode.AlreadyLoggedIn, (refused as LoginResult.Refused).error.code)
        }
        withTimeout(5.seconds) { session.closed.await() }
        assertEquals(emptyList(), session.incoming().toList(), "the flow ends with the session")
        Unit
    }
}
