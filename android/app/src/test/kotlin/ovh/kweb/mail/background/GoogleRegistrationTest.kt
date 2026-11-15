package ovh.kweb.mail.background

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class GoogleRegistrationTest {
    @Test
    fun theRegistrationCarriesEveryExtraNameOldAndNew() {
        val e = GoogleRegistration.extras("acct1", "KEY", "wp:https://mail.example/#id", delete = false)
        assertEquals("KEY", e["sender"])
        assertEquals("KEY", e["subscription"])
        assertEquals("KEY", e["X-subscription"])
        assertEquals("wp:https://mail.example/#id", e["subtype"])
        assertEquals("wp:https://mail.example/#id", e["X-subtype"])
        assertEquals("GCM", e["scope"])
        assertEquals("|ID|acct1|", e["kid"])
        assertFalse(e.containsKey("delete"))
    }

    @Test
    fun anUnregisterAddsDelete() {
        assertEquals("1", GoogleRegistration.extras("a", "K", "s", delete = true)["delete"])
    }

    @Test
    fun aTokenReplyNamesItsAccountThroughTheKid() {
        val r = GoogleRegistration.parseReply("|ID|acct1|:abc-DEF_1:2", null, "ovh.kweb.mail")
        assertEquals(RegistrationReply.Token("acct1", "abc-DEF_1:2"), r)
    }

    @Test
    fun aTokenEqualToThePackageNameAnswersAnUnregister() {
        assertEquals(RegistrationReply.Unregistered("acct1"), GoogleRegistration.parseReply("|ID|acct1|:ovh.kweb.mail", null, "ovh.kweb.mail"))
    }

    @Test
    fun anErrorIsARefusalWithOrWithoutAnAccount() {
        assertEquals(RegistrationReply.Error(null, "SERVICE_NOT_AVAILABLE"), GoogleRegistration.parseReply(null, "SERVICE_NOT_AVAILABLE", "p"))
        assertEquals(RegistrationReply.Error("a", "X"), GoogleRegistration.parseReply("|ID|a|:", "X", "p"))
    }

    @Test
    fun anErrorThatCarriesTheKidNamesItsAccount() {
        assertEquals(RegistrationReply.Error("acct1", "SERVICE_NOT_AVAILABLE"), GoogleRegistration.parseReply(null, "|ID|acct1|:SERVICE_NOT_AVAILABLE", "p"))
        // So it reaches only that account's request, never the one of another account that happens to wait.
        val err = RegistrationReply.Error("acct1", "E")
        assertNull(GoogleRegistration.answerFor(err, "acct2", delete = false))
        assertEquals(RegistrationResult.Refused("E"), GoogleRegistration.answerFor(err, "acct1", delete = false))
    }

    @Test
    fun anErrorWithoutAnAccountRightAfterAnUnansweredRequestIsNotTrusted() {
        val bare = RegistrationReply.Error(null, "SERVICE_NOT_AVAILABLE")
        assertFalse("no request was left unanswered", GoogleRegistration.mayBeStray(bare, null))
        assertTrue(GoogleRegistration.mayBeStray(bare, 0))
        assertTrue(GoogleRegistration.mayBeStray(bare, GoogleRegistration.STRAY_WINDOW_MS - 1))
        assertFalse(GoogleRegistration.mayBeStray(bare, GoogleRegistration.STRAY_WINDOW_MS))
        // What names its account is always delivered.
        assertFalse(GoogleRegistration.mayBeStray(RegistrationReply.Error("a", "E"), 0))
        assertFalse(GoogleRegistration.mayBeStray(RegistrationReply.Token("a", "t"), 0))
    }

    @Test
    fun nonsenseIsUnknown() {
        assertEquals(RegistrationReply.Unknown, GoogleRegistration.parseReply(null, null, "p"))
        assertEquals(RegistrationReply.Unknown, GoogleRegistration.parseReply("abc", null, "p"))
        assertEquals(RegistrationReply.Unknown, GoogleRegistration.parseReply("|ID|a|:", null, "p"))
    }

    @Test
    fun aReplyReachesOnlyTheAccountItIsFor() {
        val token = RegistrationReply.Token("a", "tok")
        assertEquals(RegistrationResult.Token("tok"), GoogleRegistration.answerFor(token, "a", delete = false))
        assertNull(GoogleRegistration.answerFor(token, "b", delete = false))
        assertNull(GoogleRegistration.answerFor(token, "a", delete = true))
        assertEquals(RegistrationResult.Token(""), GoogleRegistration.answerFor(RegistrationReply.Unregistered("a"), "a", delete = true))
        assertNull(GoogleRegistration.answerFor(RegistrationReply.Unregistered("a"), "a", delete = false))
    }

    @Test
    fun anErrorWithoutAnAccountGoesToTheWaiter() {
        val err = RegistrationReply.Error(null, "E")
        assertEquals(RegistrationResult.Refused("E"), GoogleRegistration.answerFor(err, "a", delete = false))
        assertNull(GoogleRegistration.answerFor(RegistrationReply.Error("b", "E"), "a", delete = false))
    }
}
