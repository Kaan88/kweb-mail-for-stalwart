package ovh.kweb.mail.ui.compose

import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test
import uniffi.kweb_core.Address

class CompletionRulesTest {
    private val john = listOf(Address("John Smith", "john@example.com"))
    private val en = Locale.ENGLISH

    private fun at(text: String, recipients: List<Address> = john, name: String = "Kaan") =
        CompletionRules.suggest(text, text.length, text.length, recipients, name, en)

    // ---- greeting ----

    @Test fun nameAfterGreeting() {
        val s = at("Hi ")!!
        assertEquals(CompletionRules.Kind.Name, s.kind)
        assertEquals("John", s.replacement)
        assertEquals("Hi John", CompletionRules.apply("Hi ", s).first)
        assertEquals(7, CompletionRules.apply("Hi ", s).second)
    }

    @Test fun turkishGreetingsAndSecondLine() {
        assertEquals("John", at("Merhaba ")!!.replacement)
        assertEquals("John", at("Sayın ")!!.replacement)
        assertEquals("John", at("Intro line\n\nDear ")!!.replacement)
    }

    @Test fun partialNameIsCompletedAndReplaced() {
        val text = "Hello Jo"
        val s = at(text)!!
        assertEquals("Hello John", CompletionRules.apply(text, s).first)
    }

    @Test fun nothingOnceTheNameIsComplete() {
        assertNull(at("Hi John"))
        assertNull(at("Hi Jane"))
    }

    @Test fun greetingNeedsTheSpaceAndTheLineStart() {
        assertNull(at("Hi"))
        assertNull(at("Say hi "))
        assertNull(at("  Hi "))
    }

    @Test fun lastCommaFirstDisplayName() {
        assertEquals("Ayşe", at("Hi ", listOf(Address("Yılmaz, Ayşe", "a@b.c")))!!.replacement)
    }

    @Test fun roleAddressesAreNeverGuessed() {
        assertNull(at("Hi ", listOf(Address("", "info@example.com"))))
        assertNull(at("Hi ", listOf(Address("", "noreply@example.com"))))
        assertNull(at("Hi ", listOf(Address("", "john@example.com"))))
        assertNull(at("Hi ", listOf(Address("Acme Support", "x@example.com"))))
        assertNull(at("Hi ", listOf(Address("", "sales.team@example.com"))))
        assertNull(at("Hi ", emptyList()))
    }

    @Test fun clearFirstDotLastAddressIsAGuess() {
        assertEquals("Mary", at("Hi ", listOf(Address("", "mary.jones@example.com")))!!.replacement)
        assertNull(at("Hi ", listOf(Address("", "mary99.jones@example.com"))))
    }

    @Test fun onlyTheFirstRecipient() {
        val two = listOf(Address("Zed Zee", "z@e.c"), Address("John Smith", "j@e.c"))
        assertEquals("Zed", at("Hi ", two)!!.replacement)
    }

    // ---- sign-off ----

    @Test fun signOffWithName() {
        val text = "Thanks for everything.\n\nBest"
        val s = at(text)!!
        assertEquals(CompletionRules.Kind.SignOff, s.kind)
        assertEquals("Best regards,", s.label)
        assertEquals("Thanks for everything.\n\nBest regards,\nKaan", CompletionRules.apply(text, s).first)
    }

    @Test fun signOffWithoutNameWhenUnknown() {
        val text = "Text.\nKind"
        assertEquals("Text.\nKind regards,", CompletionRules.apply(text, at(text, name = "")!!).first)
    }

    @Test fun signatureBelowIsNotDuplicated() {
        val text = "Text.\nBest\n-- \nKaan Y."
        val s = CompletionRules.suggest(text, 10, 10, john, "Kaan", en)!!
        assertEquals("Best regards,", s.replacement)
    }

    @Test fun turkishSignOffs() {
        assertEquals("İyi günler,", at("Metin.\nİyi")!!.label)
        assertEquals("İyi günler,", at("Metin.\niyi g")!!.label)
        assertEquals("Saygılarımla,", at("Metin.\nSaygıl")!!.label)
    }

    @Test fun otherSignOffs() {
        assertEquals("Thanks,", at("x\nThanks")!!.label)
        assertEquals("Sincerely,", at("x\nSincere")!!.label)
        assertEquals("Cheers,", at("x\nCheers")!!.label)
        assertEquals("Warm regards,", at("x\nWarm")!!.label)
        assertEquals("Regards,", at("x\nReg")!!.label)
    }

    @Test fun signOffDismissedWhenTypingMovesOn() {
        assertNull(at("x\nThanks for"))
        assertNull(at("x\nBest regards,"))
        assertNull(at("x\nBe"))
    }

    @Test fun noSignOffOnTheFirstLine() {
        assertNull(at("Best"))
    }

    // ---- where nothing is offered ----

    @Test fun neverWithASelection() {
        assertNull(CompletionRules.suggest("Hi ", 0, 3, john, "Kaan", en))
    }

    @Test fun neverInQuotedText() {
        assertNull(at("> Hi "))
        assertNull(at("Hello\n\nOn Mon, 3 Nov 2026, John wrote:\nHi "))
        assertNull(at("Hello\n\nJohn tarihinde yazdı:\nBest"))
        assertNull(at("x\n---------- Forwarded message ---------\nBest"))
    }

    @Test fun neverMidLine() {
        val text = "Hi \nmore"
        assertNotNull(CompletionRules.suggest(text, 3, 3, john, "Kaan", en))
        assertNull(CompletionRules.suggest("Hi there", 3, 3, john, "Kaan", en))
    }

    @Test fun badCursorIsIgnored() {
        assertNull(CompletionRules.suggest("Hi ", 9, 9, john, "Kaan", en))
        assertNull(CompletionRules.suggest("Hi ", -1, -1, john, "Kaan", en))
    }
}
