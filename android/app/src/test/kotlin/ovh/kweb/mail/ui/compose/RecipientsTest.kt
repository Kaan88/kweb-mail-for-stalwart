package ovh.kweb.mail.ui.compose

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.Address

class RecipientsTest {
    private fun addr(email: String, name: String = "") = Address(name, email)

    // ---- validity: the core's rule ----

    @Test fun aPlainAddressIsValid() = assertTrue(Recipients.isValid("a@b.c"))

    @Test fun exactlyOneAtSign() {
        assertFalse(Recipients.isValid("ab.c"))
        assertFalse(Recipients.isValid("a@b@c"))
    }

    @Test fun thePartsOnBothSidesMustNotBeEmpty() {
        assertFalse(Recipients.isValid("@b.c"))
        assertFalse(Recipients.isValid("a@"))
        assertFalse(Recipients.isValid("@"))
        assertFalse(Recipients.isValid(""))
    }

    @Test fun noBlanksAnywhere() {
        assertFalse(Recipients.isValid("a b@c.d"))
        assertFalse(Recipients.isValid(" a@b.c"))
        assertFalse(Recipients.isValid("a@b.c "))
        assertFalse(Recipients.isValid("a@b.c\n"))
    }

    @Test fun theCoreAcceptsADomainWithoutADot() = assertTrue(Recipients.isValid("root@localhost"))

    // ---- parse ----

    @Test fun parseTakesAPlainAddress() = assertEquals(addr("a@b.c"), Recipients.parse("  a@b.c  "))

    @Test fun parseTakesNameAndAngleBrackets() =
        assertEquals(addr("ayse@x.org", "Ayşe Demir"), Recipients.parse("Ayşe Demir <ayse@x.org>"))

    @Test fun parseDropsQuotesAroundTheName() = assertEquals(addr("d@x.org", "Doe, John"), Recipients.parse("\"Doe, John\" <d@x.org>"))

    @Test fun parseTakesBracketsWithoutAName() = assertEquals(addr("a@b.c"), Recipients.parse("<a@b.c>"))

    @Test fun parseRefusesWhatIsNotAnAddress() {
        assertNull(Recipients.parse("Ayşe"))
        assertNull(Recipients.parse("Ayşe <ayse>"))
        assertNull(Recipients.parse("Ayşe <ayse@x.org"))
        assertNull(Recipients.parse(""))
    }

    // ---- when typed text becomes chips ----

    @Test fun textWithoutAFinishingKeyStaysText() {
        val c = Recipients.consume("a@b.c")
        assertEquals(emptyList<Address>(), c.accepted)
        assertEquals("a@b.c", c.rest)
        assertFalse(c.rejected)
    }

    @Test fun aCommaFinishesAnAddress() {
        val c = Recipients.consume("a@b.c,")
        assertEquals(listOf(addr("a@b.c")), c.accepted)
        assertEquals("", c.rest)
    }

    @Test fun aSemicolonFinishesAnAddress() = assertEquals(listOf(addr("a@b.c")), Recipients.consume("a@b.c;").accepted)

    @Test fun aBlankAfterAFullAddressFinishesIt() {
        val c = Recipients.consume("a@b.c ")
        assertEquals(listOf(addr("a@b.c")), c.accepted)
        assertEquals("", c.rest)
    }

    @Test fun aBlankAfterAWordThatIsNoAddressIsJustABlank() {
        val c = Recipients.consume("Ayşe ")
        assertEquals(emptyList<Address>(), c.accepted)
        assertEquals("Ayşe ", c.rest)
        assertFalse(c.rejected)
    }

    @Test fun enterFinishesAnAddress() {
        val c = Recipients.consume("a@b.c", enter = true)
        assertEquals(listOf(addr("a@b.c")), c.accepted)
        assertEquals("", c.rest)
    }

    @Test fun enterOnSomethingThatIsNoAddressKeepsItAndSaysSo() {
        val c = Recipients.consume("sel", enter = true)
        assertEquals(emptyList<Address>(), c.accepted)
        assertEquals("sel", c.rest)
        assertTrue(c.rejected)
    }

    @Test fun enterOnNothingDoesNothing() {
        val c = Recipients.consume("  ", enter = true)
        assertEquals(emptyList<Address>(), c.accepted)
        assertEquals("", c.rest)
        assertFalse(c.rejected)
    }

    @Test fun aSeparatorAfterSomethingThatIsNoAddressKeepsItAsText() {
        val c = Recipients.consume("sel,")
        assertEquals(emptyList<Address>(), c.accepted)
        assertEquals("sel", c.rest)
        assertTrue(c.rejected)
    }

    @Test fun manyAddressesAtOnceInThePastedOrder() {
        val c = Recipients.consume("a@b.c, d@e.f; g@h.i")
        assertEquals(listOf(addr("a@b.c"), addr("d@e.f")), c.accepted)
        assertEquals("g@h.i", c.rest)
    }

    @Test fun theWalkStopsAtTheFirstPartThatIsNoAddress() {
        val c = Recipients.consume("a@b.c, oops, d@e.f,")
        assertEquals(listOf(addr("a@b.c")), c.accepted)
        assertEquals("oops, d@e.f", c.rest)
        assertTrue(c.rejected)
    }

    @Test fun aCommaInsideQuotesIsPartOfTheName() {
        val c = Recipients.consume("\"Doe, John\" <d@x.org>,")
        assertEquals(listOf(addr("d@x.org", "Doe, John")), c.accepted)
    }

    @Test fun aCommaInsideAngleBracketsDoesNotSplit() {
        val c = Recipients.consume("Name <a@b.c>, ")
        assertEquals(listOf(addr("a@b.c", "Name")), c.accepted)
        assertEquals("", c.rest)
    }

    @Test fun emptyPartsBetweenSeparatorsAreSkipped() {
        val c = Recipients.consume(",, a@b.c,")
        assertEquals(listOf(addr("a@b.c")), c.accepted)
        assertEquals("", c.rest)
    }

    // ---- merging, marking, blocking ----

    @Test fun anAddressAlreadyThereIsNotAddedAgainWhateverTheCase() {
        val merged = Recipients.merge(listOf(addr("A@b.c")), listOf(addr("a@B.C"), addr("d@e.f"), addr("d@e.f")))
        assertEquals(listOf(addr("A@b.c"), addr("d@e.f")), merged)
    }

    @Test fun leftoverTextThatIsNotAnAddressBlocksSending() {
        assertTrue(Recipients.blocksSend("sel"))
        assertFalse(Recipients.blocksSend(""))
        assertFalse(Recipients.blocksSend("   "))
        assertFalse(Recipients.blocksSend("a@b.c"))
    }

    @Test fun textIsMarkedOnlyAfterATryToFinishIt() {
        assertFalse(Recipients.isMarked("sel", rejectedBefore = false))
        assertTrue(Recipients.isMarked("sel", rejectedBefore = true))
        assertFalse(Recipients.isMarked("a@b.c", rejectedBefore = true))
        assertFalse(Recipients.isMarked("", rejectedBefore = true))
    }
}
