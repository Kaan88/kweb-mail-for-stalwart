package ovh.kweb.mail.ui.signin

import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeoutOrNull
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class SignInReturnTest {
    @Test
    fun anOlderSignInEndingDoesNotSwitchOffANewerOne() = runBlocking {
        val older = SignInReturn.expect()
        val newer = SignInReturn.expect()
        SignInReturn.stopExpecting(older)
        SignInReturn.deliver("ovh.kweb.mail:/oauth/callback?state=x")
        assertEquals("ovh.kweb.mail:/oauth/callback?state=x", withTimeoutOrNull(1000) { SignInReturn.await() })
        SignInReturn.stopExpecting(newer)
    }

    @Test
    fun anAnswerWithNobodyWaitingIsDropped() = runBlocking {
        val token = SignInReturn.expect()
        SignInReturn.stopExpecting(token)
        SignInReturn.deliver("ovh.kweb.mail:/oauth/callback?state=y")
        assertNull(withTimeoutOrNull(200) { SignInReturn.await() })
        SignInReturn.acknowledgeInterrupted()
    }
}
