package ovh.kweb.mail.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AccountRulesTest {
    private val base = record("a", user = "Kaan", server = "https://mail.kweb.ovh", jmap = "d1")

    @Test
    fun theSameServerAndTheSameJmapAccountIsADuplicate() {
        assertTrue(AccountRules.isDuplicate(listOf(base), record("b", user = "other", server = "https://mail.kweb.ovh", jmap = "d1")))
    }

    @Test
    fun theSameServerAndTheSameLoginWithoutRegardToCaseIsADuplicate() {
        assertTrue(AccountRules.isDuplicate(listOf(base), record("b", user = "kaan", server = "https://mail.kweb.ovh", jmap = "d9")))
    }

    @Test
    fun aDifferentLoginOnTheSameServerIsNot() {
        assertFalse(AccountRules.isDuplicate(listOf(base), record("b", user = "bob", server = "https://mail.kweb.ovh", jmap = "d2")))
    }

    @Test
    fun theSameLoginOnAnotherServerIsNot() {
        assertFalse(AccountRules.isDuplicate(listOf(base), record("b", user = "Kaan", server = "https://mail.example.org", jmap = "d1")))
    }

    @Test
    fun aDifferentPortIsAnotherServerAndTheDefaultPortIsTheSame() {
        assertFalse(AccountRules.isDuplicate(listOf(base), record("b", user = "Kaan", server = "https://mail.kweb.ovh:8443", jmap = "d1")))
        assertTrue(AccountRules.isDuplicate(listOf(base), record("b", user = "Kaan", server = "https://MAIL.kweb.ovh:443/", jmap = "d1")))
    }

    @Test
    fun anAccountIsNotADuplicateOfItself() {
        assertFalse(AccountRules.isDuplicate(listOf(base), base))
    }

    @Test
    fun normaliseServerKeepsASchemeAndHostAndDropsTheRest() {
        assertEquals("https://mail.kweb.ovh", AccountRules.normaliseServer(" HTTPS://Mail.Kweb.OVH:443/jmap?x=1 "))
        assertEquals("http://localhost:8080", AccountRules.normaliseServer("http://localhost:8080/"))
        assertEquals("not an address", AccountRules.normaliseServer("Not An Address/"))
    }
}
