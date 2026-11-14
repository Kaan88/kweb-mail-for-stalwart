package ovh.kweb.mail.ui.compose

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.Address
import uniffi.kweb_core.Attachment
import uniffi.kweb_core.Draft

class ComposeRulesTest {
    private fun draft(
        body: String = "",
        subject: String = "",
        to: List<Address> = emptyList(),
        attachments: List<Attachment> = emptyList(),
        source: String? = null,
        forward: Boolean = false,
        serverId: String? = null,
        sendError: String? = null,
    ) = Draft("d1", "i1", to, emptyList(), emptyList(), subject, body, attachments, source, forward, serverId, sendError, 0)

    private val someone = Address("", "a@b.c")
    private val file = Attachment("blob", "a.pdf", "application/pdf", 10u, null, false)

    @Test fun aDraftWithNoSourceIsANewMessage() = assertEquals(ComposeRules.Kind.New, ComposeRules.kind(draft()))

    @Test fun aDraftWithASourceIsAReplyUnlessItForwards() {
        assertEquals(ComposeRules.Kind.Reply, ComposeRules.kind(draft(source = "m1")))
        assertEquals(ComposeRules.Kind.Forward, ComposeRules.kind(draft(source = "m1", forward = true)))
    }

    @Test fun contentIgnoresTheCoresBookkeeping() {
        val a = draft(body = "x")
        assertTrue(ComposeRules.sameContent(a, a.copy(serverId = "s", updatedAt = 99, sendError = "no")))
    }

    @Test fun contentSeesEveryFieldTheUserEdits() {
        val a = draft(body = "x")
        assertFalse(ComposeRules.sameContent(a, a.copy(body = "y")))
        assertFalse(ComposeRules.sameContent(a, a.copy(subject = "s")))
        assertFalse(ComposeRules.sameContent(a, a.copy(to = listOf(someone))))
        assertFalse(ComposeRules.sameContent(a, a.copy(cc = listOf(someone))))
        assertFalse(ComposeRules.sameContent(a, a.copy(bcc = listOf(someone))))
        assertFalse(ComposeRules.sameContent(a, a.copy(identityId = "i2")))
        assertFalse(ComposeRules.sameContent(a, a.copy(attachments = listOf(file))))
    }

    @Test fun anUntouchedBlankNewMessageIsDiscarded() {
        val opened = draft()
        assertTrue(ComposeRules.discardsOnLeave(opened, opened))
    }

    @Test fun anUntouchedNewMessageWithOnlyASignatureIsDiscarded() {
        val opened = draft(body = "\n-- \nKaan")
        assertTrue(ComposeRules.discardsOnLeave(opened, opened.copy()))
    }

    @Test fun aNewMessageThatWasWrittenInIsKept() {
        val opened = draft()
        assertFalse(ComposeRules.discardsOnLeave(opened, opened.copy(body = "hi")))
        assertFalse(ComposeRules.discardsOnLeave(opened, opened.copy(to = listOf(someone))))
        assertFalse(ComposeRules.discardsOnLeave(opened, opened.copy(attachments = listOf(file))))
    }

    @Test fun aDraftThatHadContentWhenItOpenedIsNeverDiscarded() {
        assertFalse(ComposeRules.discardsOnLeave(draft(subject = "s"), draft(subject = "s")))
        assertFalse(ComposeRules.discardsOnLeave(draft(to = listOf(someone)), draft(to = listOf(someone))))
        assertFalse(ComposeRules.discardsOnLeave(draft(attachments = listOf(file)), draft(attachments = listOf(file))))
    }

    @Test fun anAnswerIsKeptEvenUntouched() =
        assertFalse(ComposeRules.discardsOnLeave(draft(source = "m1"), draft(source = "m1")))

    @Test fun aDraftThatIsOnTheServerOrFailedToSendIsKept() {
        assertFalse(ComposeRules.discardsOnLeave(draft(serverId = "s"), draft(serverId = "s")))
        assertFalse(ComposeRules.discardsOnLeave(draft(sendError = "refused"), draft(sendError = "refused")))
    }

    @Test fun theTimings() {
        assertEquals(2_000L, ComposeRules.AUTOSAVE_MILLIS)
        assertEquals(10u, ComposeRules.UNDO_SECONDS)
        assertEquals(300L, ComposeRules.SUGGEST_MILLIS)
    }

    @Test fun undoIsOfferedForWhatIsLeftOfTheHoldBack() {
        assertEquals(10_000L, SendRules.remainingMillis(sendAtSeconds = 110, nowMillis = 100_000))
        assertEquals(4_000L, SendRules.remainingMillis(sendAtSeconds = 104, nowMillis = 100_000))
        assertEquals(0L, SendRules.remainingMillis(sendAtSeconds = 99, nowMillis = 100_000))
    }

    @Test fun aClockThatIsOffDoesNotKeepTheSnackbarForMinutes() =
        assertEquals(10_000L, SendRules.remainingMillis(sendAtSeconds = 100 + 600, nowMillis = 100_000))
}
