package ovh.kweb.mail.accounts

import org.junit.Assert.assertEquals
import org.junit.Test
import uniffi.kweb_core.ConnectionStatus

class RegisteredStatusTest {
    @Test
    fun anAccountInTheListWithNoSessionNeedsASignIn() {
        assertEquals(ConnectionStatus.SignInNeeded, forRegistered(ConnectionStatus.SignedOut))
        assertEquals(ConnectionStatus.Connecting, forRegistered(ConnectionStatus.Connecting))
    }
}
