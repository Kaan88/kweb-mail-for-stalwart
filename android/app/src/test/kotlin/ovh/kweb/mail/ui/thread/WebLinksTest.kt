package ovh.kweb.mail.ui.thread

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class WebLinksTest {
    private fun found(text: String) = WebLinks.find(text).map { text.substring(it) }

    @Test fun findsWebAddressesInText() {
        assertEquals(listOf("https://sferastudios.com/tutorial/post/2032"), found("link: https://sferastudios.com/tutorial/post/2032 Netflix"))
        assertEquals(listOf("www.example.org/a?b=1"), found("see www.example.org/a?b=1."))
        assertEquals(listOf("http://x.test/p"), found("(http://x.test/p)"))
        assertEquals(listOf("https://en.wikipedia.org/wiki/Foo_(bar)"), found("https://en.wikipedia.org/wiki/Foo_(bar), ok"))
    }

    @Test fun leavesOtherThingsAlone() {
        assertEquals(emptyList<String>(), found("mail me at me@www.example.org or ftp://x.test or javascript:alert(1)"))
        assertEquals(emptyList<String>(), found("https:// and www."))
    }

    @Test fun manyClosingBracketsAndLongTextsStayFast() {
        val brackets = "http://x.test/" + ")".repeat(1_000)
        assertEquals(listOf("http://x.test/"), found(brackets))
        assertEquals(emptyList<String>(), found("http://x.test/" + "a".repeat(5_000)))
        val log = (1..3_000).joinToString(" ") { "https://h$it.test/p" }
        val started = System.nanoTime()
        assertEquals(3_000, WebLinks.find(log, html = true).size)
        assertTrue(System.nanoTime() - started < 2_000_000_000L)
    }

    @Test fun anEndTagIsMatchedByItsWholeName() {
        val out = WebLinks.html("""<a href="https://a.test"><abbr>x</abbr> https://b.test</a>""")
        assertEquals("""<a href="https://a.test"><abbr>x</abbr> https://b.test</a>""", out)
    }

    @Test fun wwwGetsHttps() {
        assertEquals("https://www.example.org", WebLinks.href("www.example.org"))
        assertEquals("http://a.test", WebLinks.href("http://a.test"))
    }

    @Test fun linksTextInHtmlOnly() {
        val html = """<p class="https://no">Go to https://a.test/x&amp;y=1&nbsp;now</p><a href="https://b.test">https://b.test</a>""" +
            """<style>a{background:url(https://c.test)}</style><!-- https://d.test --><img alt="www.e.test">"""
        val out = WebLinks.html(html)
        assertEquals(
            """<p class="https://no">Go to <a href="https://a.test/x&amp;y=1">https://a.test/x&amp;y=1</a>&nbsp;now</p>""" +
                """<a href="https://b.test">https://b.test</a><style>a{background:url(https://c.test)}</style>""" +
                """<!-- https://d.test --><img alt="www.e.test">""",
            out,
        )
    }

    @Test fun aMailWithoutAddressesIsTheSameString() {
        val html = "<p>Hello</p>"
        assertTrue(WebLinks.html(html) === html)
    }

    @Test fun unclosedPartsAreFast() {
        val hostile = "<a>" + "https://x.test ".repeat(20_000) + "<style>" + "<p '".repeat(20_000)
        val start = System.nanoTime()
        WebLinks.html(hostile)
        assertTrue((System.nanoTime() - start) / 1_000_000 < 2_000)
    }
}
