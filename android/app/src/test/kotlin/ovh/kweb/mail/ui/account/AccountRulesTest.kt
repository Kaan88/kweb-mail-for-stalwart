package ovh.kweb.mail.ui.account

import java.time.ZoneId
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.AccountFeatures
import uniffi.kweb_core.ConnectedApp
import uniffi.kweb_core.MaskedAddress

class AccountRulesTest {
    private fun features(masked: Boolean = false, security: Boolean = false, again: Boolean = false) =
        AccountFeatures(edition = "enterprise", maskedEmail = masked, security = security, needsSignIn = again)

    @Test fun nothingOfferedShowsNothing() = assertEquals(FeaturesRow.None, AccountRules.row(features()))

    @Test fun whatIsOfferedIsListed() {
        assertEquals(FeaturesRow.Offered(masked = true, security = false), AccountRules.row(features(masked = true)))
        assertEquals(FeaturesRow.Offered(masked = false, security = true), AccountRules.row(features(security = true)))
        assertEquals(FeaturesRow.Offered(masked = true, security = true), AccountRules.row(features(masked = true, security = true)))
    }

    @Test fun anOldSignInAsksToSignInAgain() = assertEquals(FeaturesRow.SignInAgain, AccountRules.row(features(again = true)))

    @Test fun datesAreWrittenInTheUsersLocale() {
        assertEquals("Nov 14, 2023", AccountRules.date(1_700_000_000L, Locale.US, ZoneId.of("UTC")))
        assertNull(AccountRules.date(null, Locale.US))
    }

    @Test fun aShortDateDropsTheYearOnlyInTheCurrentYear() {
        val utc = ZoneId.of("UTC")
        val now = java.time.Instant.ofEpochSecond(1_700_100_000L)
        assertEquals("Nov 14", AccountRules.shortDate(1_700_000_000L, Locale.US, utc, now))
        assertEquals("Nov 14, 2023", AccountRules.shortDate(1_700_000_000L, Locale.US, utc, java.time.Instant.ofEpochSecond(1_800_000_000L)))
        assertNull(AccountRules.shortDate(null, Locale.US, utc, now))
    }

    @Test fun anEndDateInThePastIsEnded() {
        assertTrue(AccountRules.isEnded(100, 200))
        assertFalse(AccountRules.isEnded(300, 200))
        assertFalse(AccountRules.isEnded(null, 200))
    }
}

class MaskedRulesTest {
    private fun a(id: String, on: Boolean = true) = MaskedAddress(id, "$id@example.org", "", "", on, null, null)

    @Test fun theLimitCountsEverything() {
        assertTrue(MaskedRules.atLimit(5, 5))
        assertTrue(MaskedRules.atLimit(6, 5))
        assertFalse(MaskedRules.atLimit(4, 5))
        assertFalse(MaskedRules.atLimit(99, null))
    }

    @Test fun aPrefixIsLettersDigitsDotHyphenUnderscore() {
        assertTrue(MaskedRules.prefixIsValid(""))
        assertTrue(MaskedRules.prefixIsValid(" shop-1.a_b "))
        assertFalse(MaskedRules.prefixIsValid("sh op"))
        assertFalse(MaskedRules.prefixIsValid("shop@"))
        assertFalse(MaskedRules.prefixIsValid("é"))
        assertTrue(MaskedRules.prefixIsValid("a".repeat(32)))
        assertFalse(MaskedRules.prefixIsValid("a".repeat(33)))
    }

    @Test fun aSiteLosesSchemeAndPath() {
        assertEquals("example.com", MaskedRules.cleanSite(" https://example.com/login?x=1 "))
        assertEquals("example.com", MaskedRules.cleanSite("example.com"))
        assertEquals("", MaskedRules.cleanSite("  "))
    }

    @Test fun thePurposeJoinsNoteAndSite() {
        assertEquals("Bookshop · example.com", MaskedRules.purpose("Bookshop ", "example.com"))
        assertEquals("example.com", MaskedRules.purpose("", "example.com"))
        assertNull(MaskedRules.purpose(" ", ""))
    }

    @Test fun onlyTheNamedSwitchChanges() {
        val list = MaskedRules.withEnabled(listOf(a("x"), a("y")), "y", false)
        assertEquals(listOf(true, false), list.map { it.enabled })
    }
}

class SecurityRulesTest {
    private fun app(vararg scopes: String, own: Boolean = false) = ConnectedApp("1", "App", scopes.toList(), null, own)

    @Test fun expiryChoicesBecomeUnixSeconds() {
        assertNull(SecurityRules.expiresAt(Expiry.NEVER, 1000))
        assertEquals(1000 + 30 * 86_400L, SecurityRules.expiresAt(Expiry.DAYS_30, 1000))
        assertEquals(1000 + 365 * 86_400L, SecurityRules.expiresAt(Expiry.YEAR_1, 1000))
    }

    @Test fun scopeNamesBecomeWords() {
        val parts = SecurityRules.scopes(listOf("mail", "calendars", "contacts", "files", "settings", "security", "urn:stalwart:oauth:scope:full", "offline_access"))
        assertEquals(
            listOf(ScopeWord.MAIL, ScopeWord.CALENDARS, ScopeWord.CONTACTS, ScopeWord.FILES, ScopeWord.SETTINGS, ScopeWord.SECURITY, ScopeWord.FULL, ScopeWord.STAY_SIGNED_IN).map { ScopePart.Known(it) },
            parts,
        )
    }

    @Test fun anUnknownScopeIsShownAsItIs() {
        assertEquals(listOf(ScopePart.Known(ScopeWord.MAIL), ScopePart.Raw("tickets")), SecurityRules.scopes(listOf("mail", "mail", " ", "tickets")))
    }

    @Test fun passwordFormProblemsInOrder() {
        assertEquals(PasswordProblem.CURRENT_EMPTY, SecurityRules.passwordProblem("", "a", "a"))
        assertEquals(PasswordProblem.NEW_EMPTY, SecurityRules.passwordProblem("old", "", ""))
        assertEquals(PasswordProblem.MISMATCH, SecurityRules.passwordProblem("old", "a", "b"))
        assertNull(SecurityRules.passwordProblem("old", "a", "a"))
    }

    @Test fun theMismatchShowsOnlyOnceTheSecondFieldIsFilled() {
        assertFalse(SecurityRules.showsMismatch("abc", ""))
        assertTrue(SecurityRules.showsMismatch("abc", "ab"))
        assertFalse(SecurityRules.showsMismatch("abc", "abc"))
    }

    @Test fun aCodeIsSixDigits() {
        assertTrue(SecurityRules.codeIsComplete("123456"))
        assertTrue(SecurityRules.codeIsComplete("123 456"))
        assertFalse(SecurityRules.codeIsComplete("12345"))
        assertFalse(SecurityRules.codeIsComplete("12345a"))
        assertEquals("123456", SecurityRules.cleanCode("123 456"))
    }

    @Test fun aSecretForTheClipboardHasNoSpaces() = assertEquals("ABCD1234", SecurityRules.plainSecret("ABCD 1234"))

    @Test fun thisAppIsNotRevoked() {
        assertFalse(SecurityRules.mayRevoke(app("mail", own = true)))
        assertTrue(SecurityRules.mayRevoke(app("mail")))
    }

    @Test fun theLastPasskeyIsSaidStronger() {
        assertTrue(SecurityRules.isLastPasskey(1))
        assertFalse(SecurityRules.isLastPasskey(2))
    }
}
