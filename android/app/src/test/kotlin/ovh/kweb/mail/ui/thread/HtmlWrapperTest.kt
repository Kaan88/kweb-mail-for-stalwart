package ovh.kweb.mail.ui.thread

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class HtmlWrapperTest {
    private val page = HtmlWrapper.wrap("<p>Hello <b>wörld</b></p>")

    @Test fun theMailIsInsideTheBodyUnchanged() = assertTrue(page.contains("<body><p>Hello <b>wörld</b></p></body>"))

    @Test fun thePolicyForbidsScriptsPluginsFormsAndBase() = assertEquals(
        "script-src 'none'; object-src 'none'; form-action 'none'; base-uri 'none'; frame-src 'none'; media-src 'none'",
        HtmlWrapper.POLICY,
    )

    @Test fun thePolicyComesBeforeTheMail() {
        val policy = page.indexOf("Content-Security-Policy")
        assertTrue(policy in 0 until page.indexOf("<body>"))
        assertTrue(page.contains("content=\"${HtmlWrapper.POLICY}\""))
    }

    @Test fun hasAViewportAndCharset() {
        assertTrue(page.contains("<meta name=\"viewport\" content=\"width=device-width"))
        assertTrue(page.contains("<meta charset=\"utf-8\">"))
    }

    @Test fun wideMailKeepsItsLayoutAndOnlyLooseThingsAreHeldIn() {
        assertTrue(page.contains("img{max-width:100%;height:auto}"))
        assertTrue(page.contains("pre{white-space:pre-wrap;overflow-wrap:break-word}"))
        // Tables are scaled with the page, not squeezed: squeezing broke newsletters made of fixed-width tables.
        assertTrue(!page.contains("table,"))
        assertTrue(!page.contains("td,th{"))
    }

    @Test fun theViewportLetsTheViewStartAtTheScaleThatFitsAndAllowsPinchZoom() {
        assertTrue(page.contains("<meta name=\"viewport\" content=\"width=device-width, maximum-scale=5, user-scalable=yes\">"))
        // Either would stop overview mode from starting at the scale where a wide mail fits.
        assertTrue(!page.contains("initial-scale"))
        assertTrue(!page.contains("minimum-scale"))
    }

    @Test fun foldedQuotesHaveTheirStyleAndNeedNoScript() {
        assertTrue(page.contains("details.kq>summary"))
        assertTrue(!page.contains("<script"))
    }

    @Test fun aMailWithItsOwnDocumentIsStillAfterThePolicy() {
        val wrapped = HtmlWrapper.wrap("<html><head><meta http-equiv=\"refresh\" content=\"0;url=http://x\"></head><body>hi</body></html>")
        assertTrue(wrapped.indexOf("Content-Security-Policy") < wrapped.indexOf("refresh"))
    }

    @Test fun theMailsOwnViewportTagIsRemoved() {
        val wrapped = HtmlWrapper.wrap("<meta name=\"viewport\" content=\"width=600, user-scalable=no\"><p>hi</p><META NAME='Viewport' content='x'>")
        assertTrue(wrapped.contains("<p>hi</p>"))
        assertTrue(!wrapped.contains("width=600"))
        assertTrue(!wrapped.contains("content='x'"))
        assertTrue(wrapped.contains(HtmlWrapper.VIEWPORT))
    }

    @Test fun metaTagsThatNeverEndAreFast() {
        val t = System.nanoTime()
        HtmlWrapper.wrap("<p>x</p>" + "<meta ".repeat(150_000))
        assertTrue((System.nanoTime() - t) / 1_000_000 < 10_000)
    }

    @Test fun repeatedViewportNamesInOneTagAreFast() {
        val t = System.nanoTime()
        HtmlWrapper.wrap("<meta " + "name=viewport ".repeat(70_000))
        HtmlWrapper.wrap("<meta " + "name=viewport ".repeat(70_000) + ">")
        assertTrue((System.nanoTime() - t) / 1_000_000 < 10_000)
    }

    @Test fun turnsDnsPrefetchOffBeforeTheMail() {
        val meta = "<meta http-equiv=\"x-dns-prefetch-control\" content=\"off\">"
        assertTrue(page.contains(meta))
        assertTrue(page.indexOf(meta) < page.indexOf("<body>"))
    }

    @Test fun aSecondLayoutIsAsWideAsTheMail() {
        val wide = HtmlWrapper.wrap("<p>x</p>", layoutWidth = 560)
        assertTrue(wide.contains("<meta name=\"viewport\" content=\"width=560, maximum-scale=5, user-scalable=yes\">"))
        assertTrue(!wide.contains("initial-scale") && !wide.contains("minimum-scale"))
        // Neither layout touches the mail's own text-size-adjust: the size comes from the view's text zoom.
        assertTrue(!wide.contains("text-size-adjust") && !page.contains("text-size-adjust"))
        assertEquals(HtmlWrapper.VIEWPORT, HtmlWrapper.viewport(null))
        assertEquals(HtmlWrapper.VIEWPORT, HtmlWrapper.viewport(0))
    }
}
