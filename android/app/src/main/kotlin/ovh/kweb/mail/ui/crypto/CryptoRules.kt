package ovh.kweb.mail.ui.crypto

import uniffi.kweb_core.CryptoContent
import uniffi.kweb_core.CryptoKind
import uniffi.kweb_core.CryptoView
import uniffi.kweb_core.EncryptionState
import uniffi.kweb_core.MessageBody
import uniffi.kweb_core.SignatureState

/**
 * What the screens decide about a checked message, without drawing anything. The core says what the
 * signature and the encryption of a message are; these rules say how much good news that is and what
 * the screen may do with the message (show its own copy, load remote content, ask for a passphrase).
 */
object CryptoRules {
    enum class Tone { GOOD, NEUTRAL, WARNING, BAD }

    /** Only a valid signature from the From address, from a key the user has, is good news. */
    fun signatureTone(s: SignatureState): Tone? = when (s) {
        SignatureState.NOT_SIGNED -> null
        SignatureState.VERIFIED -> Tone.GOOD
        SignatureState.KEY_CHANGED, SignatureState.OTHER_ADDRESS, SignatureState.KEY_EXPIRED, SignatureState.KEY_REVOKED -> Tone.WARNING
        SignatureState.CERTIFICATE_NOT_CHECKED, SignatureState.FIRST_SEEN_KEY, SignatureState.NO_KEY, SignatureState.UNSUPPORTED, SignatureState.UNREADABLE -> Tone.NEUTRAL
        SignatureState.INVALID -> Tone.BAD
    }

    fun encryptionTone(e: EncryptionState): Tone? = when (e) {
        EncryptionState.NOT_ENCRYPTED -> null
        EncryptionState.DECRYPTED -> Tone.GOOD
        EncryptionState.NO_INTEGRITY -> Tone.BAD
        else -> Tone.NEUTRAL
    }

    /**
     * The worst of the two. A decrypted message that is not signed is not good news about who wrote it, so it
     * is neutral: opening it says only that it was meant for you.
     */
    fun tone(v: CryptoView): Tone {
        val parts = listOfNotNull(signatureTone(v.signature), encryptionTone(v.encryption))
        if (v.signature == SignatureState.NOT_SIGNED && v.encryption == EncryptionState.DECRYPTED) return Tone.NEUTRAL
        return when {
            Tone.BAD in parts -> Tone.BAD
            Tone.WARNING in parts -> Tone.WARNING
            Tone.NEUTRAL in parts -> Tone.NEUTRAL
            parts.isEmpty() -> Tone.NEUTRAL
            else -> Tone.GOOD
        }
    }

    /** A status chip is shown for signed and encrypted mail only. */
    fun showsChip(kind: CryptoKind): Boolean = kind != CryptoKind.NONE

    /** The message is shown from the core's own copy (decrypted or taken out of opaque signed data) and not from the server's body. */
    fun contentOverride(v: CryptoView?): CryptoContent? = v?.content

    /**
     * Remote content loads only when the user allowed it for the message AND the core did not forbid it (S/MIME mail,
     * whose encryption cannot tell a changed message from an original one).
     */
    fun remoteAllowed(userAllowed: Boolean, v: CryptoView?): Boolean = userAllowed && v?.remoteContentBlocked != true

    /** Whether the "show pictures" bar may be offered at all. */
    fun remoteOffered(v: CryptoView?): Boolean = v?.remoteContentBlocked != true

    /** The body the card draws: the server's, or, for mail the core opened, the core's copy alone (its files are drawn apart). */
    fun displayBody(server: MessageBody, v: CryptoView?): MessageBody {
        val content = contentOverride(v) ?: return server
        return server.copy(html = content.html, text = content.text, attachments = emptyList(), isTruncated = false)
    }

    fun needsPassphrase(v: CryptoView): Boolean =
        v.encryption == EncryptionState.NEED_PASSPHRASE || v.encryption == EncryptionState.WRONG_PASSPHRASE

    /**
     * A passphrase prompt belongs to the page the user is on: a neighbour in the pager that is only loaded
     * must not put a dialog on the screen. [settled] is true for the shown page.
     */
    fun mayPrompt(v: CryptoView, settled: Boolean): Boolean = settled && needsPassphrase(v)

    /** The key a prompt is about, in short. */
    fun keyName(v: CryptoView): String = v.passphraseFor?.takeIf { it.isNotBlank() } ?: ""

    /** A signed message whose status covers only part of it must say so. */
    fun coversEverything(v: CryptoView): Boolean = v.coversWholeMessage
}
