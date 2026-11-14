package ovh.kweb.mail.ui.thread

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.ui.thread.UrlPolicy.Fetch
import ovh.kweb.mail.ui.thread.UrlPolicy.Link

class UrlPolicyTest {
    private val blockedRemote = Fetch.Empty(remote = true)
    private val blockedOther = Fetch.Empty(remote = false)

    @Test fun remoteContentIsRefusedAndReportedByDefault() {
        assertEquals(blockedRemote, UrlPolicy.fetch("https://tracker.example/p.gif", allowRemote = false))
        assertEquals(blockedRemote, UrlPolicy.fetch("http://tracker.example/p.gif", allowRemote = false))
    }

    @Test fun remoteContentLoadsWhenAllowed() {
        assertEquals(Fetch.Load, UrlPolicy.fetch("https://x.example/a.png", allowRemote = true))
        assertEquals(Fetch.Load, UrlPolicy.fetch("HTTP://x.example/a.png", allowRemote = true))
    }

    @Test fun cidNamesAnInlinePicture() {
        assertEquals(Fetch.Inline("logo@example.org"), UrlPolicy.fetch("cid:logo@example.org", allowRemote = false))
        assertEquals(Fetch.Inline("logo@example.org"), UrlPolicy.fetch("CID:logo@example.org", allowRemote = true))
    }

    @Test fun cidIsDecodedAndLosesAngleBrackets() {
        assertEquals(Fetch.Inline("a b@x"), UrlPolicy.fetch("cid:a%20b%40x", allowRemote = false))
        assertEquals(Fetch.Inline("özet"), UrlPolicy.fetch("cid:%C3%B6zet", allowRemote = false))
        assertEquals(Fetch.Inline("id@x"), UrlPolicy.fetch("cid:<id@x>", allowRemote = false))
    }

    @Test fun everyOtherSchemeGetsAnEmptyAnswerEvenWhenRemoteIsAllowed() {
        for (url in listOf("file:///etc/passwd", "content://x/y", "data:image/png;base64,AAAA", "javascript:alert(1)", "ftp://x/y", "ws://x", "about:blank", "intent://x#Intent;end")) {
            assertEquals(url, blockedOther, UrlPolicy.fetch(url, allowRemote = true))
        }
    }

    @Test fun anAddressWithoutASchemeOrAStrangeOneGetsAnEmptyAnswer() {
        for (url in listOf("", "x.png", "//x.example/a.png", " https://x.example/a", "ht tp://x", ":foo")) {
            assertEquals(url, blockedOther, UrlPolicy.fetch(url, allowRemote = true))
        }
    }

    @Test fun webAndMailLinksOpenOutsideTheApp() {
        assertEquals(Link.Open("https://example.org/a?b=c"), UrlPolicy.link("https://example.org/a?b=c"))
        assertEquals(Link.Open("http://example.org"), UrlPolicy.link("http://example.org"))
        assertEquals(Link.Open("mailto:a@example.org"), UrlPolicy.link("mailto:a@example.org"))
        assertEquals(Link.Open("HTTPS://example.org"), UrlPolicy.link("HTTPS://example.org"))
    }

    @Test fun otherLinksAreIgnored() {
        for (url in listOf("javascript:alert(1)", "file:///x", "content://x", "intent://x#Intent;end", "tel:123", "cid:x", "#top", "", "ht tp://x")) {
            assertEquals(url, Link.Ignore, UrlPolicy.link(url))
        }
    }

    @Test fun percentDecodingLeavesBrokenEscapesAlone() {
        assertEquals("100%", UrlPolicy.percentDecode("100%"))
        assertEquals("%zz", UrlPolicy.percentDecode("%zz"))
        assertEquals("a+b", UrlPolicy.percentDecode("a+b"))
        assertEquals("%-1", UrlPolicy.percentDecode("%-1"))
        assertEquals("𝐀", UrlPolicy.percentDecode("𝐀"))
    }

    @Test fun onlyMailtoIsAMailLink() {
        assertTrue(UrlPolicy.isMail("mailto:a@example.org"))
        assertTrue(UrlPolicy.isMail("MAILTO:a@example.org"))
        assertFalse(UrlPolicy.isMail("https://example.org"))
        assertFalse(UrlPolicy.isMail("a@example.org"))
    }
}
