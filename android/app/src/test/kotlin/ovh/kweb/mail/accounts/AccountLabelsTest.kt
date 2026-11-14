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
}
