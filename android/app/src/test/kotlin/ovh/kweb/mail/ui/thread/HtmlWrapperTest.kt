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

    @Test fun wideMailIsMadeToFit() {
        assertTrue(page.contains("img,table{max-width:100%;height:auto}"))
        assertTrue(page.contains("overflow-wrap:anywhere"))
    }

    @Test fun aMailWithItsOwnDocumentIsStillAfterThePolicy() {
        val wrapped = HtmlWrapper.wrap("<html><head><meta http-equiv=\"refresh\" content=\"0;url=http://x\"></head><body>hi</body></html>")
        assertTrue(wrapped.indexOf("Content-Security-Policy") < wrapped.indexOf("refresh"))
    }

    @Test fun turnsDnsPrefetchOffBeforeTheMail() {
        val meta = "<meta http-equiv=\"x-dns-prefetch-control\" content=\"off\">"
        assertTrue(page.contains(meta))
        assertTrue(page.indexOf(meta) < page.indexOf("<body>"))
    }
}
