package ovh.kweb.mail.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class RegistryCodecTest {
    @Test
    fun aStateSurvivesTheRoundTrip() {
        val state = RegistryState(
            accounts = listOf(record("a1", legacy = true), record("a2", user = "bob, the \"second\"")),
            active = "a2",
            discard = setOf("a3", "a4:legacy"),
        )
        assertEquals(state, RegistryCodec.decode(RegistryCodec.encode(state)))
    }

    @Test
    fun anEmptyStateSurvivesToo() {
        assertEquals(RegistryState(), RegistryCodec.decode(RegistryCodec.encode(RegistryState())))
    }

    @Test
    fun nothingStoredIsNoStateAndSoIsAMapWithoutAVersion() {
        assertNull(RegistryCodec.decode(null))
        assertNull(RegistryCodec.decode(emptyMap()))
        assertNull(RegistryCodec.decode(mapOf("active" to "a1")))
    }

    @Test
    fun unknownKeysAreIgnoredAndADamagedAccountIsSkipped() {
        val values = RegistryCodec.encode(RegistryState(accounts = listOf(record("a1"), record("a2")), active = "a2")).toMutableMap()
        values["something.new"] = "x"
        values.remove("a.0.id")
        val state = RegistryCodec.decode(values)!!
        assertEquals(listOf("a2"), state.accounts.map { it.id })
        assertEquals("a2", state.active)
    }

    @Test
    fun anActiveIdThatIsNotInTheListIsDropped() {
        val values = RegistryCodec.encode(RegistryState(accounts = listOf(record("a1")), active = "a1")).toMutableMap()
        values["active"] = "ghost"
        assertNull(RegistryCodec.decode(values)!!.active)
    }
}
