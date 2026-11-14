package ovh.kweb.mail.ui.mailextras

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.R
import uniffi.kweb_core.Unsubscribe

class UnsubscribeRulesTest {
    private fun info(oneClick: Boolean = false, byMail: Boolean = false, web: String? = null) = Unsubscribe(oneClick, byMail, web)

    @Test fun nothingUsableMeansNoWay() {
        assertNull(UnsubscribeRules.way(null))
        assertNull(UnsubscribeRules.way(info()))
    }

    @Test fun theCoresOwnWaysComeBeforeTheBrowser() {
        assertEquals(UnsubscribeRules.Way.REQUEST, UnsubscribeRules.way(info(oneClick = true, byMail = true, web = "https://x.example/u")))
        assertEquals(UnsubscribeRules.Way.MAIL, UnsubscribeRules.way(info(byMail = true, web = "https://x.example/u")))
        assertEquals(UnsubscribeRules.Way.BROWSER, UnsubscribeRules.way(info(web = "https://x.example/u")))
    }

    @Test fun onlyAnHttpsPageGoesToTheBrowser() {
        assertNull(UnsubscribeRules.way(info(web = "http://x.example/u")))
        assertNull(UnsubscribeRules.way(info(web = "javascript:alert(1)")))
        assertNull(UnsubscribeRules.way(info(web = "https://")))
        assertFalse(UnsubscribeRules.isSafeWebUrl("https://x.example/a b"))
        assertTrue(UnsubscribeRules.isSafeWebUrl("HTTPS://x.example/u"))
    }

    @Test fun eachWayHasItsOwnWords() {
        val texts = UnsubscribeRules.Way.entries.map { UnsubscribeRules.confirmText(it) }
        assertEquals(3, texts.toSet().size)
        assertEquals(R.string.unsub_open_page, UnsubscribeRules.confirmButton(UnsubscribeRules.Way.BROWSER))
        assertEquals(R.string.unsub_confirm_button, UnsubscribeRules.confirmButton(UnsubscribeRules.Way.MAIL))
    }
}
