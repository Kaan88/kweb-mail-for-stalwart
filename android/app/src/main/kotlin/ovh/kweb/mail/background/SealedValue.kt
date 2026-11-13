package ovh.kweb.mail.background

import java.util.Base64

/**
 * An AES-GCM result as one string: base64 of the IV, a colon, base64 of the ciphertext.
 * Kept apart from [KeystoreSecrets] so the format can be tested without Android.
 */
internal class SealedValue(val iv: ByteArray, val ciphertext: ByteArray) {
    fun encode(): String =
        Base64.getEncoder().encodeToString(iv) + SEPARATOR + Base64.getEncoder().encodeToString(ciphertext)

    companion object {
        private const val SEPARATOR = ":"

        /** Null when [text] is not in the format [encode] writes. */
        fun decode(text: String): SealedValue? {
            val parts = text.split(SEPARATOR)
            if (parts.size != 2 || parts[0].isEmpty() || parts[1].isEmpty()) return null
            return try {
                SealedValue(Base64.getDecoder().decode(parts[0]), Base64.getDecoder().decode(parts[1]))
            } catch (_: IllegalArgumentException) {
                null
            }
        }
    }
}
