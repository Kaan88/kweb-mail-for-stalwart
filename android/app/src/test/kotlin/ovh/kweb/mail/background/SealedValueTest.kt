package ovh.kweb.mail.background

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class SealedValueTest {
    @Test
    fun encodingRoundTrips() {
        val sealed = SealedValue(byteArrayOf(1, 2, 3), byteArrayOf(9, 8, 7, 6))
        val back = SealedValue.decode(sealed.encode())
        assertNotNull(back)
        assertArrayEquals(sealed.iv, back!!.iv)
        assertArrayEquals(sealed.ciphertext, back.ciphertext)
    }

    @Test
    fun damagedTextIsRejectedNotReadAsEmpty() {
        assertNull(SealedValue.decode(""))
        assertNull(SealedValue.decode("no-separator"))
        assertNull(SealedValue.decode("AAAA:"))
        assertNull(SealedValue.decode("a:b:c"))
        assertNull(SealedValue.decode("!!!:???"))
    }
}
