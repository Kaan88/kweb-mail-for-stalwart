package ovh.kweb.mail.background

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class UpdateRulesTest {
    @Test fun aNewerVersionIsAnnouncedOnceNotAgainForTheSameOne() {
        assertTrue(UpdateRules.shouldNotify(newer = true, latest = "0.2.0", alreadyNotified = ""))
        assertTrue(UpdateRules.shouldNotify(newer = true, latest = "0.3.0", alreadyNotified = "0.2.0"))
        assertFalse(UpdateRules.shouldNotify(newer = true, latest = "0.2.0", alreadyNotified = "0.2.0"))
    }

    @Test fun nothingIsAnnouncedWhenThereIsNothingNewerOrNoVersion() {
        assertFalse(UpdateRules.shouldNotify(newer = false, latest = "0.1.0", alreadyNotified = ""))
        assertFalse(UpdateRules.shouldNotify(newer = true, latest = "", alreadyNotified = ""))
    }

    @Test fun theBrowserOpensOnlyAGitHubPage() {
        assertTrue(UpdateRules.safePage("https://github.com/Kaan88/kweb-mail-for-stalwart/releases/tag/v0.2.0"))
        assertFalse(UpdateRules.safePage("http://github.com/x"))
        assertFalse(UpdateRules.safePage("https://evil.example/releases"))
        assertFalse(UpdateRules.safePage("https://github.com.evil.example/x"))
        assertEquals(8, UpdateRules.NOTIFICATION_ID)
    }
}
