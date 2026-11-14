package ovh.kweb.mail.ui.admin

import java.time.ZoneId
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.LogLine
import uniffi.kweb_core.ServerAccountKind

class AdminRulesTest {
    @Test fun aKeyIsShownWithItsFirstEightCharactersAndDots() {
        val shown = AdminRules.maskKey("abcdefgh12345678ijklmnop")
        assertTrue(shown.startsWith("abcdefgh"))
        assertEquals("abcdefgh" + "•".repeat(16), shown)
    }

    @Test fun aShortKeyNeverShowsMoreThanHalf() {
        assertEquals("abcd" + "•".repeat(4), AdminRules.maskKey("abcdefgh"))
        assertEquals("", AdminRules.maskKey("").trim('•'))
    }

    @Test fun aVeryLongKeyShowsABoundedNumberOfDots() {
        assertEquals(8 + 24, AdminRules.maskKey("k".repeat(300)).length)
    }

    @Test fun theClipboardGivesAKeyOnlyWhenItLooksLikeOne() {
        assertEquals("abc123", AdminRules.keyFromClipboard("  abc123\n"))
        assertNull(AdminRules.keyFromClipboard(null))
        assertNull(AdminRules.keyFromClipboard("   "))
        assertNull(AdminRules.keyFromClipboard("two words"))
        assertNull(AdminRules.keyFromClipboard("API_12\u000134"))
        assertNull(AdminRules.keyFromClipboard("API_12\u007f34"))
        assertNull(AdminRules.keyFromClipboard("API_12\n34"))
        assertNull(AdminRules.keyFromClipboard("x".repeat(513)))
    }

    @Test fun theAccessIsLockedRefusedOrOpen() {
        assertEquals(AccessKind.Open, AdminRules.accessKind(access(), false))
        assertEquals(AccessKind.Locked, AdminRules.accessKind(access(connected = false), false))
        assertEquals(AccessKind.Locked, AdminRules.accessKind(null, false))
        assertEquals(AccessKind.Refused, AdminRules.accessKind(access(connected = false, refused = true), false))
        assertEquals(AccessKind.Refused, AdminRules.accessKind(access(connected = false), true))
    }

    @Test fun aLicensedServerListsTheEnterpriseAreasTheKeyMayOpen() {
        val a = access(edition = "enterprise", enterprise = true).copy(mayAlerts = false, mayTenants = false)
        val areas = AdminRules.homeAreas(a)
        assertEquals(listOf(AdminArea.Dashboard, AdminArea.History, AdminArea.Recovery), areas.lower)
        assertFalse(areas.lowerDimmed)
        assertEquals(5, areas.upper.size)
    }

    @Test fun aServerWithoutALicenceShowsAllFiveDimmed() {
        val areas = AdminRules.homeAreas(access(edition = "community"))
        assertEquals(5, areas.lower.size)
        assertTrue(areas.lowerDimmed)
        assertTrue(AdminRules.homeAreas(access(edition = "")).lowerDimmed)
    }

    @Test fun aBuildWithoutTheEnterprisePartsLeavesTheLowerGroupOut() {
        assertTrue(AdminRules.homeAreas(access(edition = "oss")).lower.isEmpty())
    }

    @Test fun aKeyThatMayDoLessListsOnlyThoseRows() {
        val a = access(all = false).copy(mayQueue = true)
        assertEquals(listOf(AdminArea.Queue), AdminRules.homeAreas(a).upper)
        val rows = AdminRules.mayRows(a)
        assertTrue(rows.first { it.kind == MayKind.Queue }.allowed)
        assertFalse(rows.first { it.kind == MayKind.Accounts }.allowed)
        assertFalse(rows.first { it.kind == MayKind.Enterprise }.allowed)
    }

    @Test fun anAccountNameHasLettersDigitsDotHyphenUnderscore() {
        assertNull(AdminRules.nameProblem("ada.lovelace_1-x"))
        assertEquals(NameProblem.Empty, AdminRules.nameProblem(""))
        assertEquals(NameProblem.BadCharacters, AdminRules.nameProblem("a b"))
        assertEquals(NameProblem.BadCharacters, AdminRules.nameProblem("ada@example.org"))
        assertEquals(NameProblem.BadCharacters, AdminRules.nameProblem("é"))
        assertEquals(NameProblem.BadCharacters, AdminRules.nameProblem(".ada"))
        assertEquals(NameProblem.BadCharacters, AdminRules.nameProblem("a".repeat(65)))
    }

    @Test fun aPasswordNeedsEightCharactersTypedTwice() {
        assertEquals(PasswordProblem.Empty, AdminRules.passwordProblem("", ""))
        assertEquals(PasswordProblem.TooShort, AdminRules.passwordProblem("short", "short"))
        assertEquals(PasswordProblem.Different, AdminRules.passwordProblem("longenough1", "longenough2"))
        assertNull(AdminRules.passwordProblem("longenough1", "longenough1"))
    }

    @Test fun deleteWantsTheNameTyped() {
        assertTrue(AdminRules.deleteConfirmed("bob", "bob"))
        assertTrue(AdminRules.deleteConfirmed(" Bob ", "bob"))
        assertFalse(AdminRules.deleteConfirmed("bo", "bob"))
        assertFalse(AdminRules.deleteConfirmed("", ""))
    }

    @Test fun quotaTextIsReadAsGigabytesByDefault() {
        assertEquals(QuotaInput.NoLimit, AdminRules.parseQuota("  "))
        assertEquals(QuotaInput.Limit(5L * 1024 * 1024 * 1024), AdminRules.parseQuota("5"))
        assertEquals(QuotaInput.Limit(500L * 1024 * 1024), AdminRules.parseQuota("500 MB"))
        assertEquals(QuotaInput.Limit((1.5 * 1024 * 1024 * 1024).toLong()), AdminRules.parseQuota("1,5gb"))
        assertEquals(QuotaInput.Invalid, AdminRules.parseQuota("lots"))
        assertEquals(QuotaInput.Invalid, AdminRules.parseQuota("5 parsecs"))
        assertEquals(QuotaInput.Invalid, AdminRules.parseQuota("-3"))
    }

    @Test fun quotaTextRoundTrips() {
        assertEquals("", AdminRules.quotaText(null))
        assertEquals("5 GB", AdminRules.quotaText(5uL * 1024uL * 1024uL * 1024uL))
        assertEquals("500 MB", AdminRules.quotaText(500uL * 1024uL * 1024uL))
        assertEquals(QuotaInput.Limit(500L * 1024 * 1024), AdminRules.parseQuota(AdminRules.quotaText(500uL * 1024uL * 1024uL)))
    }

    @Test fun bytesAreWrittenInTheUnitsPeopleUse() {
        assertEquals("48 KB", AdminRules.formatBytes(48uL * 1024uL, Locale.ENGLISH))
        assertEquals("1.5 GB", AdminRules.formatBytes(1536uL * 1024uL * 1024uL, Locale.ENGLISH))
        assertEquals("12 B", AdminRules.formatBytes(12uL, Locale.ENGLISH))
    }

    @Test fun theBarFillsWithTheUseAndWarnsFromNinetyPercent() {
        assertNull(AdminRules.usedFraction(null, 100u))
        assertNull(AdminRules.usedFraction(5u, null))
        assertNull(AdminRules.usedFraction(5u, 0u))
        assertEquals(0.5f, AdminRules.usedFraction(50u, 100u)!!, 0.001f)
        assertEquals(1f, AdminRules.usedFraction(500u, 100u)!!, 0.001f)
        assertTrue(AdminRules.nearlyFull(95u, 100u))
        assertFalse(AdminRules.nearlyFull(50u, 100u))
    }

    @Test fun anAccountsPills() {
        assertEquals(listOf(AccountPill.Group), AdminRules.pills(account("s", kind = ServerAccountKind.GROUP)))
        assertEquals(listOf(AccountPill.Suspended), AdminRules.pills(account("i", suspended = true)))
        assertEquals(listOf(AccountPill.Admin), AdminRules.pills(account("a").copy(isAdmin = true)))
        assertTrue(AdminRules.pills(account("b")).isEmpty())
        assertEquals("A", AdminRules.initial("ada"))
        assertEquals("?", AdminRules.initial(""))
    }

    @Test fun theCoresWordsForARecipientBecomeFourStates() {
        assertEquals(RecipientState.Waiting, AdminRules.recipientState("waiting"))
        assertEquals(RecipientState.WillRetry, AdminRules.recipientState("failed for now"))
        assertEquals(RecipientState.Delivered, AdminRules.recipientState("delivered"))
        assertEquals(RecipientState.Failed, AdminRules.recipientState("failed"))
        assertEquals(RecipientState.Other, AdminRules.recipientState("odd"))
    }

    @Test fun aTryThatIsPastOrDueIsNotShownAsAClockTime() {
        assertTrue(AdminRules.isDue(946_684_800, 1_800_000_000))
        assertTrue(AdminRules.isDue(1_800_000_010, 1_800_000_000))
        assertFalse(AdminRules.isDue(1_800_000_600, 1_800_000_000))
    }

    @Test fun spansRoundUpToWholeMinutesHoursAndDays() {
        assertEquals(Span(SpanUnit.Minutes, 1), AdminRules.span(-5))
        assertEquals(Span(SpanUnit.Minutes, 1), AdminRules.span(61 - 60))
        assertEquals(Span(SpanUnit.Minutes, 4), AdminRules.span(200))
        assertEquals(Span(SpanUnit.Hours, 2), AdminRules.span(61 * 60))
        assertEquals(Span(SpanUnit.Days, 3), AdminRules.span(3L * 86_400))
    }

    @Test fun theTryLine() {
        assertEquals(TryLine.NotTried, AdminRules.tryLine(0u, null, 0))
        assertEquals(TryLine.NotTried, AdminRules.tryLine(0u, 100, 0))
        assertEquals(TryLine.Tried(3, Span(SpanUnit.Minutes, 4)), AdminRules.tryLine(3u, 240, 0))
        assertEquals(TryLine.Tried(3, null), AdminRules.tryLine(3u, null, 0))
    }

    @Test fun theQueueLine() {
        assertEquals(QueueLine.Empty, AdminRules.queueLine(0, emptyList(), 0, paused = false))
        assertEquals(QueueLine.Paused(3), AdminRules.queueLine(3, listOf(60L), 0, paused = true))
        assertEquals(QueueLine.Waiting(3, Span(SpanUnit.Minutes, 1)), AdminRules.queueLine(3, listOf(null, 300L, 60L), 0, paused = false))
        assertEquals(QueueLine.Waiting(2, null), AdminRules.queueLine(2, listOf(null, null), 0, paused = false))
    }

    @Test fun recipientsAreSummedAsTheFirstAndTheRest() {
        assertEquals("a" to 2, AdminRules.recipientsSummary(listOf("a", "b", "c")))
        assertEquals("" to 0, AdminRules.recipientsSummary(emptyList()))
    }

    @Test fun logLevelsNarrowTheLoadedLines() {
        val e = LogLine(1L, "error", "x")
        val w = LogLine(2L, "warning", "y")
        val i = LogLine(3L, "info", "z")
        assertEquals(listOf(e, w, i), listOf(e, w, i).filter { AdminRules.matches(LevelFilter.All, it) })
        assertEquals(listOf(e), listOf(e, w, i).filter { AdminRules.matches(LevelFilter.Errors, it) })
        assertEquals(listOf(w), listOf(e, w, i).filter { AdminRules.matches(LevelFilter.Warnings, it) })
        assertEquals(601, AdminRules.clip("x".repeat(900)).length)
    }

    @Test fun aTimeIsTheClockForTodayAndTheDateForOtherDays() {
        val zone = ZoneId.of("UTC")
        val now = 1_700_000_000L
        val sameDay = AdminFormat.whenText(now - 600, now, zone, Locale.ENGLISH)
        val otherDay = AdminFormat.whenText(now - 3 * 86_400, now, zone, Locale.ENGLISH)
        assertTrue(otherDay.length > sameDay.length)
    }
}
