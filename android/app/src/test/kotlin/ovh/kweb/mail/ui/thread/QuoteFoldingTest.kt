package ovh.kweb.mail.ui.thread

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class QuoteFoldingTest {
    private val label = "Quoted text"

    @Test fun aConversationOfThreeFoldsEveryQuote() {
        assertFalse(QuoteFolding.foldAll(2))
        assertTrue(QuoteFolding.foldAll(3))
    }

    @Test fun aBlockquoteIsWrappedAndNothingIsLost() {
        val html = "<p>Thanks!</p><blockquote>old <b>text</b></blockquote>"
        val out = QuoteFolding.foldHtml(html, label, foldAll = true)
        assertEquals(
            "<p>Thanks!</p><details class=\"kq\"><summary>… Quoted text</summary><blockquote>old <b>text</b></blockquote></details>",
            out,
        )
    }

    @Test fun aGmailQuoteDivIsWrappedWithItsNestedDivs() {
        val html = "<div>Hi</div><div class=\"gmail_quote gmail_quote_container\"><div class=\"gmail_attr\">On Mon, A wrote:<br></div><div>inner</div></div>"
        val out = QuoteFolding.foldHtml(html, label, foldAll = true)
        assertTrue(out.contains("<details class=\"kq\"><summary>… Quoted text</summary><div class=\"gmail_quote gmail_quote_container\">"))
        assertTrue(out.contains("<div>inner</div></div></details>"))
    }

    @Test fun onlyTheOutermostQuoteIsWrapped() {
        val html = "<p>x</p><blockquote>a<blockquote>b</blockquote>c</blockquote>"
        val out = QuoteFolding.foldHtml(html, label, foldAll = true)
        assertEquals(1, Regex("<details").findAll(out).count())
        assertTrue(out.contains("<blockquote>a<blockquote>b</blockquote>c</blockquote></details>"))
    }

    @Test fun aQuoteWithTextAfterItStaysOpenInALongConversation() {
        val html = "<p>Hi</p><blockquote>pull quote</blockquote><p>and more</p>"
        assertEquals(html, QuoteFolding.foldHtml(html, label, foldAll = true))
    }

    @Test fun manyUnclosedQuotesAreFast() {
        val html = "<p>x</p>" + "<blockquote>".repeat(80_000)
        val t = System.nanoTime()
        QuoteFolding.foldHtml(html, label, foldAll = true)
        assertTrue((System.nanoTime() - t) / 1_000_000 < 10_000)
    }

    @Test fun tagsThatNeverEndAreFast() {
        for (open in listOf("<div ", "<blockquote ", "<style ", "<!-- ")) {
            val html = "<p>x</p>" + open.repeat(200_000)
            val t = System.nanoTime()
            QuoteFolding.foldHtml(html, label, foldAll = true)
            assertTrue(open, (System.nanoTime() - t) / 1_000_000 < 10_000)
        }
    }

    @Test fun repeatedQuoteClassesInOneTagAreFast() {
        val html = "<p>x</p><div class=\"" + "gmail_quote ".repeat(80_000) + ">"
        val t = System.nanoTime()
        QuoteFolding.foldHtml(html, label, foldAll = true)
        assertTrue((System.nanoTime() - t) / 1_000_000 < 10_000)
    }

    @Test fun aShortQuoteStaysOpenInAShortConversation() {
        val html = "<p>Thanks!</p><blockquote>old</blockquote>"
        assertEquals(html, QuoteFolding.foldHtml(html, label, foldAll = false))
    }

    @Test fun aLongQuoteIsFoldedEvenInAShortConversation() {
        val html = "<p>Thanks!</p><blockquote>" + "x".repeat(QuoteFolding.LONG_HTML_QUOTE) + "</blockquote>"
        assertTrue(QuoteFolding.foldHtml(html, label, foldAll = false).contains("<details"))
    }

    @Test fun aMessageThatIsOnlyAQuoteIsLeftOpen() {
        val html = "<html><head><style>p{}</style></head><body><blockquote>all of it</blockquote>&nbsp;</body></html>"
        assertEquals(html, QuoteFolding.foldHtml(html, label, foldAll = true))
    }

    @Test fun anUnclosedQuoteIsLeftAlone() {
        val html = "<p>hi</p><blockquote>never closed"
        assertEquals(html, QuoteFolding.foldHtml(html, label, foldAll = true))
    }

    @Test fun otherDivsAreNotQuotes() {
        val html = "<div class=\"signature\">me</div><div>text</div>"
        assertEquals(html, QuoteFolding.foldHtml(html, label, foldAll = true))
    }

    @Test fun theLabelIsEscaped() {
        val out = QuoteFolding.foldHtml("<p>a</p><blockquote>q</blockquote>", "<b>&", foldAll = true)
        assertTrue(out.contains("<summary>… &lt;b&gt;&amp;</summary>"))
    }

    @Test fun foldedHtmlKeepsEveryOriginalCharacter() {
        val html = "<p>a</p><blockquote>q1</blockquote><p>b</p><blockquote>q2</blockquote>"
        val out = QuoteFolding.foldHtml(html, label, foldAll = true)
        val stripped = out.replace(Regex("<details class=\"kq\"><summary>[^<]*</summary>"), "").replace("</details>", "")
        assertEquals(html, stripped)
    }

    @Test fun plainTextWithoutQuotesIsOnePart() {
        assertEquals(listOf(QuoteFolding.Part("hello\nworld", false)), QuoteFolding.splitText("hello\nworld", foldAll = true))
    }

    @Test fun plainTextQuotesAreCutOutWithTheirAttribution() {
        val text = "Sounds good.\n\nOn Mon, 3 Nov 2025 at 10:00, Ann <a@x.org> wrote:\n> first\n>\n> second\n"
        val parts = QuoteFolding.splitText(text, foldAll = true)
        assertEquals(3, parts.size)
        assertEquals(QuoteFolding.Part("Sounds good.\n", false), parts[0])
        assertEquals(QuoteFolding.Part("On Mon, 3 Nov 2025 at 10:00, Ann <a@x.org> wrote:\n> first\n>\n> second", true), parts[1])
        assertEquals(QuoteFolding.Part("", false), parts[2])
        assertEquals(text, parts.joinToString("\n") { it.text })
    }

    @Test fun aWrappedAttributionBelongsToTheQuote() {
        val text = "ok\nOn Mon, 3 Nov 2025 at 10:00, Ann Example\n<a@x.org> wrote:\n> q"
        val parts = QuoteFolding.splitText(text, foldAll = true)
        assertEquals(listOf(false, true), parts.map { it.quoted })
        assertEquals("On Mon, 3 Nov 2025 at 10:00, Ann Example\n<a@x.org> wrote:\n> q", parts[1].text)
    }

    @Test fun aShortPlainQuoteStaysOpenUnlessTheConversationIsLong() {
        val text = "ok\n> a\n> b"
        assertEquals(1, QuoteFolding.splitText(text, foldAll = false).size)
        assertEquals(2, QuoteFolding.splitText(text, foldAll = true).size)
    }

    @Test fun aLongPlainQuoteIsFoldedInAShortConversation() {
        val quote = (1..QuoteFolding.LONG_TEXT_QUOTE).joinToString("\n") { "> line $it" }
        val parts = QuoteFolding.splitText("ok\n$quote", foldAll = false)
        assertEquals(listOf(false, true), parts.map { it.quoted })
    }

    @Test fun aPlainMessageOfNothingButQuoteIsLeftOpen() {
        val text = "> a\n> b\n> c"
        assertEquals(listOf(QuoteFolding.Part(text, false)), QuoteFolding.splitText(text, foldAll = true))
    }

    @Test fun thePartsAlwaysMakeTheTextAgain() {
        val text = "a\n> q\nb\n\n> r\n\n> s\nc\n"
        assertEquals(text, QuoteFolding.splitText(text, foldAll = true).joinToString("\n") { it.text })
    }
}
