package ovh.kweb.mail.ui.assistant

import org.junit.Test
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull

class ModelFailureTest {
    @Test
    fun refusedKey() {
        assertEquals(ModelFailure.Kind.KEY_REFUSED, ModelFailure.parse("the service refused the key").kind)
    }

    @Test
    fun unreachableNamesTheHost() {
        val f = ModelFailure.parse("could not reach the service at api.example.com")
        assertEquals(ModelFailure.Kind.UNREACHABLE, f.kind)
        assertEquals("api.example.com", f.host)
    }

    @Test
    fun otherKeepsDetailsAndEmptyHasNone() {
        assertEquals("the service is busy or the quota is used up", ModelFailure.parse("the service is busy or the quota is used up").details)
        assertNull(ModelFailure.parse("").details)
    }
}
