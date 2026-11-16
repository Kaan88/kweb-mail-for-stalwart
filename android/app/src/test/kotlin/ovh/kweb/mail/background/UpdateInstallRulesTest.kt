package ovh.kweb.mail.background

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class UpdateInstallRulesTest {
    private val own = setOf("aa11")

    private fun check(pkg: String? = "ovh.kweb.mail", code: Long? = 5, signers: Set<String> = own) =
        UpdateInstallRules.refusal(pkg, code, signers, "ovh.kweb.mail", 4, own)

    @Test
    fun a_newer_build_of_this_app_signed_by_the_same_key_may_be_installed() {
        assertNull(check())
    }

    @Test
    fun another_package_an_older_or_equal_version_a_different_key_or_an_unreadable_file_is_refused() {
        assertEquals(UpdateInstallRules.Refusal.WrongApp, check(pkg = "com.evil.app"))
        assertEquals(UpdateInstallRules.Refusal.NotNewer, check(code = 4))
        assertEquals(UpdateInstallRules.Refusal.NotNewer, check(code = 3))
        assertEquals(UpdateInstallRules.Refusal.Signature, check(signers = setOf("bb22")))
        assertEquals(UpdateInstallRules.Refusal.Signature, check(signers = setOf("aa11", "bb22")))
        assertEquals(UpdateInstallRules.Refusal.Signature, check(signers = emptySet()))
        assertEquals(UpdateInstallRules.Refusal.Unreadable, check(pkg = null))
        assertEquals(UpdateInstallRules.Refusal.Unreadable, check(code = null))
        // No certificate known for the installed app: nothing can be shown equal.
        assertEquals(UpdateInstallRules.Refusal.Signature, UpdateInstallRules.refusal("ovh.kweb.mail", 5, own, "ovh.kweb.mail", 4, emptySet()))
    }

    @Test
    fun a_kept_file_must_be_newer_than_the_installed_version() {
        assertTrue(UpdateInstallRules.keep(5, 4))
        assertFalse(UpdateInstallRules.keep(4, 4))
        assertFalse(UpdateInstallRules.keep(null, 4))
    }

    @Test
    fun versions_in_file_names_are_plain_and_the_automatic_download_needs_an_unmetered_network() {
        assertTrue(UpdateInstallRules.safeVersion("0.2.1"))
        assertFalse(UpdateInstallRules.safeVersion("../x"))
        assertFalse(UpdateInstallRules.safeVersion(""))
        assertEquals("kweb-mail-0.2.1.apk", UpdateInstallRules.fileName("0.2.1"))
        assertTrue(UpdateInstallRules.mayDownloadNow(auto = true, unmetered = true))
        assertFalse(UpdateInstallRules.mayDownloadNow(auto = true, unmetered = false))
        assertFalse(UpdateInstallRules.mayDownloadNow(auto = false, unmetered = true))
    }

    @Test
    fun progress_is_a_percent_or_nothing_when_the_total_is_unknown() {
        assertEquals(50, UpdateInstallRules.percent(50, 100))
        assertEquals(100, UpdateInstallRules.percent(200, 100))
        assertNull(UpdateInstallRules.percent(5, null))
        assertNull(UpdateInstallRules.percent(5, 0))
    }
}
