package ovh.kweb.mail.ui.account

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.SecurityAccess

class SecurityNotesTest {
    @Test fun aKeyThatWasNarrowedSaysSo() = assertEquals(SecretNote.KEY_NARROWED, SecurityRules.secretNote(CredentialKind.API_KEY, true))

    @Test fun anAppPasswordThatWasNarrowedSaysSo() =
        assertEquals(SecretNote.APP_PASSWORD_NARROWED, SecurityRules.secretNote(CredentialKind.APP_PASSWORD, true))

    @Test fun aKeyWithTheAccountsRightsSaysNothing() {
        assertEquals(SecretNote.NONE, SecurityRules.secretNote(CredentialKind.API_KEY, false))
        assertEquals(SecretNote.NONE, SecurityRules.secretNote(CredentialKind.APP_PASSWORD, false))
    }

    @Test fun theNoteAboutMissingPartsShowsOnlyWhenOneIsMissing() {
        assertFalse(SecurityRules.someNotAllowed(SecurityAccess(true, true, true, true, true)))
        assertTrue(SecurityRules.someNotAllowed(SecurityAccess(true, true, false, true, true)))
        assertTrue(SecurityRules.someNotAllowed(SecurityAccess(false, false, false, false, true)))
    }
}
