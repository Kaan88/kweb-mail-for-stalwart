package ovh.kweb.mail.ui.assistant

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class DraftTextTest {
    private val quote = "Ann <ann@x.org> wrote:\n> Hello\n> there"
    private val signature = "-- \nMe\nAcme"

    private fun own(body: String, from: Int = 0, to: Int = 0): String? =
        DraftText.target(body, from, to)?.let { body.substring(it.start, it.end) }

    @Test fun aNewMessageIsAllOwnText() = assertEquals("Hello Ann", own("Hello Ann"))

    @Test fun aReplyStopsAtTheQuote() = assertEquals("Thanks, see you.", own("Thanks, see you.\n\n$quote"))

    @Test fun aReplyWithOnlyTheQuoteHasNothingToChange() = assertNull(own("\n\n$quote"))

    @Test fun theSignatureStaysOut() = assertEquals("Hello", own("Hello\n\n$signature"))

    @Test fun aReplyWithAnAnswerAndASignatureAfterTheQuote() = assertEquals("Yes.", own("Yes.\n\n$quote\n\n$signature"))

    @Test fun theSignatureBeforeAQuoteEndsItFirst() = assertEquals("Hi", own("Hi\n$signature\n\n$quote"))

    @Test fun aForwardStopsAtItsHeader() =
        assertEquals("FYI", own("FYI\n\n---------- Forwarded message ----------\nFrom: Ann <ann@x.org>\n\nbody"))

    @Test fun aLineEndingInWroteWithoutAQuoteIsOwnText() = assertEquals("He wrote:", own("He wrote:"))

    @Test fun aWroteLineFollowedByAQuoteLineWithTrailingBlanks() = assertEquals("Ok", own("Ok\nAnn wrote:  \n> hi"))

    @Test fun aQuoteIntroducedInAnotherLanguageStopsTheOwnText() =
        assertEquals("Danke", own("Danke\n\nAm 14.11.2026 schrieb Ann:\n> Hallo\n> du"))

    @Test fun anAttributionFollowedByABlankLineBeforeTheQuote() =
        assertEquals("Ok", own("Ok\n\nAnn wrote:\n\n> hi"))

    @Test fun aQuoteWithoutAnyAttributionStopsAtItsFirstLine() = assertEquals("Ok", own("Ok\n\n> hi\n> there"))

    @Test fun anOriginalMessageSeparatorEndsTheOwnText() =
        assertEquals("Ok", own("Ok\n\n-----Original Message-----\nFrom: Ann\n\nhi"))

    @Test fun aSelectionInsideAnUnusualQuoteIsCutBack() {
        val body = "One two\n\nAm 14.11. schrieb Ann:\n> Hallo"
        assertNull(own(body, body.indexOf("Hallo"), body.length))
    }

    @Test fun aLineBeforeAQuoteThatIsNoAttributionStaysOwnText() = assertEquals("Ok, see below\n\nyes", own("Ok, see below\n\nyes\n> hi"))

    @Test fun aSignatureWithoutItsTrailingSpaceIsNotOne() = assertEquals("Hi\n--\nMe", own("Hi\n--\nMe"))

    @Test fun blankSpaceAroundTheTextIsLeftAlone() {
        val body = "\n  Hello there \n\n$quote"
        val span = DraftText.target(body, 0, 0)!!
        assertEquals("Hello there", body.substring(span.start, span.end))
    }

    @Test fun aSelectionWins() {
        val body = "One two three\n\n$quote"
        assertEquals("two", own(body, 4, 7))
        assertEquals("two", own(body, 7, 4))
    }

    @Test fun aSelectionIsCutBackToTheOwnText() {
        val body = "One two\n\n$quote"
        assertEquals("two", own(body, 4, body.length))
    }

    @Test fun aSelectionInsideTheQuoteChangesNothing() {
        val body = "One two\n\n$quote"
        assertNull(own(body, body.indexOf("Hello"), body.indexOf("Hello") + 5))
    }

    @Test fun aBlankSelectionChangesNothing() = assertNull(own("a   b", 1, 4))

    @Test fun anEmptyBodyHasNothing() = assertNull(own(""))

    // ---- putting the result back ----

    @Test fun theResultTakesThePlaceOfWhatWasSent() {
        val body = "Thanks you.\n\n$quote"
        val span = DraftText.target(body, 0, 0)!!
        val done = DraftText.apply(body, span, "Thanks you.", "Thank you.\n")!!
        assertEquals("Thank you.\n\n$quote", done.body)
        assertEquals("Thank you.".length, done.cursor)
    }

    @Test fun onlyTheSelectionIsReplaced() {
        val body = "One tow three"
        val span = DraftText.target(body, 4, 7)!!
        assertEquals("One two three", DraftText.apply(body, span, "tow", "two")!!.body)
    }

    @Test fun editedTextIsFoundAgainWhenItOccursOnce() {
        val sent = "Hello Ann"
        val span = Span(0, sent.length)
        assertEquals("Hi\n\nHey there", DraftText.apply("Hi\n\nHello Ann there", span, sent, "Hey")!!.body)
    }

    @Test fun editedTextThatCannotBePlacedIsNotReplaced() {
        assertNull(DraftText.apply("something else", Span(0, 9), "Hello Ann", "Hey"))
        assertNull(DraftText.apply("Hello Ann and Hello Ann", Span(30, 39), "Hello Ann", "Hey"))
    }

    @Test fun aBlankResultReplacesNothing() = assertNull(DraftText.apply("Hello", Span(0, 5), "Hello", " \n"))

    @Test fun undoOnlyWhileTheTextIsAsTheReplacementLeftIt() {
        assertEquals("before", DraftText.undo("after", "before", "after"))
        assertNull(DraftText.undo("after, edited", "before", "after"))
    }

    // ---- the answer above the quote ----

    @Test fun theAnswerGoesAboveTheQuote() =
        assertEquals("Yes, Friday.\n\n$quote", DraftText.withAnswer("\n\n$quote", "Yes, Friday.\n"))

    @Test fun theAnswerAboveTheQuoteAndTheSignature() =
        assertEquals("Yes.\n\n$quote\n\n$signature", DraftText.withAnswer("\n\n$quote\n\n$signature", " Yes. "))

    @Test fun anEmptyBodyIsTheAnswer() = assertEquals("Yes.", DraftText.withAnswer("", "Yes."))

    @Test fun aBodyWithoutALeadingBlankLineGetsOne() = assertEquals("Yes.\n\nText", DraftText.withAnswer("Text", "Yes."))
}
