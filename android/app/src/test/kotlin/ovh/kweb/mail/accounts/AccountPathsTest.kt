package ovh.kweb.mail.accounts

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

class AccountPathsTest {
    @Test
    fun theLegacyNamesAreTodaysLiterals() {
        assertEquals("cache.db", AccountPaths.cache("x", true))
        assertEquals("secrets", AccountPaths.secretsPrefs("x", true))
        assertEquals("settings", AccountPaths.settingsPrefs("x", true))
        assertNull(AccountPaths.directory("x", true))
    }

    @Test
    fun theLegacySecretKeepsTodaysAssociatedData() {
        assertArrayEquals("refresh_token".toByteArray(), AccountPaths.secretAad("x", true, "refresh_token"))
    }

    @Test
    fun otherAccountsHaveTheirOwnFilesAndBindTheirIdIntoTheirSecrets() {
        assertEquals("accounts/ab12/cache.db", AccountPaths.cache("ab12", false))
        assertEquals("accounts/ab12", AccountPaths.directory("ab12", false))
        assertEquals("secrets.ab12", AccountPaths.secretsPrefs("ab12", false))
        assertEquals("settings.ab12", AccountPaths.settingsPrefs("ab12", false))
        assertArrayEquals("ab12\u0000refresh_token".toByteArray(), AccountPaths.secretAad("ab12", false, "refresh_token"))
        assertFalse(AccountPaths.secretAad("ab12", false, "k").contentEquals(AccountPaths.secretAad("cd34", false, "k")))
    }

    @Test
    fun blobDirectoriesAreKeyedByTheAccountAndStayInsideWhatTheFileProviderAllows() {
        assertEquals("blobs/ab12", AccountPaths.blobs("ab12"))
        assertEquals("attachments/ab12", AccountPaths.attachments("ab12"))
    }

    @Test
    fun sqliteSideFilesFollowTheCache() {
        assertEquals(listOf("cache.db-wal", "cache.db-shm", "cache.db-journal"), AccountPaths.cacheSideFiles("x", true))
    }
}
