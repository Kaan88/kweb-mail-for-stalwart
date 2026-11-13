package ovh.kweb.mail.ui.signin

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ServerAddressTest {
    @Test fun bareHostGetsHttps() = assertEquals("https://mail.kweb.ovh", ServerAddress.normalise("mail.kweb.ovh"))

    @Test fun surroundingSpacesAndSlashesAreDropped() =
        assertEquals("https://mail.kweb.ovh", ServerAddress.normalise("  https://mail.kweb.ovh/ \n"))

    @Test fun portAndPathAreKept() =
        assertEquals("https://mail.kweb.ovh:8443/x", ServerAddress.normalise("mail.kweb.ovh:8443/x"))

    @Test fun anExplicitHttpSchemeIsKeptForTheCoreToJudge() =
        assertEquals("http://localhost:8080", ServerAddress.normalise("http://localhost:8080"))

    @Test fun emptyOrBlankIsRejected() {
        assertNull(ServerAddress.normalise(""))
        assertNull(ServerAddress.normalise("   "))
        assertNull(ServerAddress.normalise("/"))
    }

    @Test fun spacesInsideAreRejected() = assertNull(ServerAddress.normalise("mail kweb.ovh"))

    @Test fun otherSchemesAreRejected() = assertNull(ServerAddress.normalise("ftp://mail.kweb.ovh"))

    @Test fun aSchemeWithoutAHostIsRejected() = assertNull(ServerAddress.normalise("https://"))
}
