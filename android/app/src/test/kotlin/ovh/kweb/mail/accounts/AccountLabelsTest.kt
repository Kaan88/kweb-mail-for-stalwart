package ovh.kweb.mail.accounts

import org.junit.Assert.assertEquals
import org.junit.Test

class AccountLabelsTest {
    @Test
    fun aLabelIsTheLogin() {
        val labels = AccountLabels.of(listOf(record("a", user = "kaan"), record("b", user = "bob")))
        assertEquals("kaan", labels["a"])
        assertEquals("bob", labels["b"])
    }

    @Test
    fun twoEqualLoginsGetTheHostToo() {
        val labels = AccountLabels.of(
            listOf(
                record("a", user = "kaan", server = "https://mail.kweb.ovh"),
                record("b", user = "Kaan", server = "https://mail.example.org:8443"),
                record("c", user = "bob"),
            ),
        )
        assertEquals("kaan · mail.kweb.ovh", labels["a"])
        assertEquals("Kaan · mail.example.org", labels["b"])
        assertEquals("bob", labels["c"])
    }

    @Test
    fun aNameTheUserGaveWinsAndIsShownEvenWithOneAccount() {
        val a = record("a", user = "kaan")
        val b = record("b", user = "bob")
        assertEquals("Work", AccountLabels.of(listOf(a, b), mapOf("a" to "Work"))["a"])
        assertEquals("bob", AccountLabels.of(listOf(a, b), mapOf("a" to "Work"))["b"])
        assertEquals("Work", AccountLabels.forNotice(a, listOf(a), mapOf("a" to "Work")))
        assertEquals(null, AccountLabels.forNotice(a, listOf(a), emptyMap()))
        assertEquals("kaan", AccountLabels.forNotice(a, listOf(a, b), emptyMap()))
        assertEquals("kaan", AccountLabels.forNotice(a, listOf(a, b), mapOf("a" to "  ")))
    }
}
