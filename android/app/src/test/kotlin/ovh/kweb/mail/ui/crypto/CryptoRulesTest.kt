package ovh.kweb.mail.ui.crypto

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.CryptoContent
import uniffi.kweb_core.CryptoKind
import uniffi.kweb_core.CryptoView
import uniffi.kweb_core.EncryptionState
import uniffi.kweb_core.MessageBody
import uniffi.kweb_core.SignatureState

class CryptoRulesTest {
    private fun view(
        signature: SignatureState = SignatureState.NOT_SIGNED,
        encryption: EncryptionState = EncryptionState.NOT_ENCRYPTED,
        content: CryptoContent? = null,
        remoteBlocked: Boolean = false,
    ) = CryptoView(
        CryptoKind.PGP_SIGNED, signature, null, "a@x.test", encryption, true, content, emptyList(), emptyList(), remoteBlocked, null, null,
    )

    private fun body() = MessageBody("m", "b", emptyList(), emptyList(), emptyList(), emptyList(), null, "<p>server</p>", "server", emptyList(), false)

    @Test fun onlyAValidSignatureFromTheFromAddressIsGoodNews() {
        assertEquals(CryptoRules.Tone.GOOD, CryptoRules.tone(view(SignatureState.VERIFIED)))
        for (s in SignatureState.values().filter { it != SignatureState.VERIFIED && it != SignatureState.NOT_SIGNED }) {
            assertTrue("$s", CryptoRules.tone(view(s)) != CryptoRules.Tone.GOOD)
        }
    }

    @Test fun aCertificateThatCameWithTheMessageIsNotCalledGood() {
        assertEquals(CryptoRules.Tone.NEUTRAL, CryptoRules.tone(view(SignatureState.CERTIFICATE_NOT_CHECKED)))
        assertEquals(CryptoRules.Tone.NEUTRAL, CryptoRules.tone(view(SignatureState.FIRST_SEEN_KEY)))
    }

    @Test fun aMismatchAndAChangedKeyAreWarningsAndAFalseSignatureIsBad() {
        assertEquals(CryptoRules.Tone.WARNING, CryptoRules.tone(view(SignatureState.OTHER_ADDRESS)))
        assertEquals(CryptoRules.Tone.WARNING, CryptoRules.tone(view(SignatureState.KEY_CHANGED)))
        assertEquals(CryptoRules.Tone.BAD, CryptoRules.tone(view(SignatureState.INVALID)))
    }

    @Test fun anOpenedMessageThatIsNotSignedIsNotGreen() {
        assertEquals(CryptoRules.Tone.NEUTRAL, CryptoRules.tone(view(encryption = EncryptionState.DECRYPTED)))
        assertEquals(CryptoRules.Tone.GOOD, CryptoRules.tone(view(SignatureState.VERIFIED, EncryptionState.DECRYPTED)))
    }

    @Test fun theWorstOfSignatureAndEncryptionDecides() {
        assertEquals(CryptoRules.Tone.BAD, CryptoRules.tone(view(SignatureState.VERIFIED, EncryptionState.NO_INTEGRITY)))
        assertEquals(CryptoRules.Tone.WARNING, CryptoRules.tone(view(SignatureState.OTHER_ADDRESS, EncryptionState.DECRYPTED)))
    }

    @Test fun onlySignedOrEncryptedMailGetsAChip() {
        assertFalse(CryptoRules.showsChip(CryptoKind.NONE))
        assertTrue(CryptoRules.showsChip(CryptoKind.SMIME_OPAQUE))
    }

    @Test fun remoteContentNeedsTheUsersYesAndTheCoresPermission() {
        assertTrue(CryptoRules.remoteAllowed(true, null))
        assertTrue(CryptoRules.remoteAllowed(true, view()))
        assertFalse(CryptoRules.remoteAllowed(false, view()))
        assertFalse(CryptoRules.remoteAllowed(true, view(remoteBlocked = true)))
        assertFalse(CryptoRules.remoteOffered(view(remoteBlocked = true)))
    }

    @Test fun theCoresCopyReplacesTheServersBodyAndTheServersFilesAreDropped() {
        val content = CryptoContent("<p>core</p>", null, emptyList())
        val shown = CryptoRules.displayBody(body(), view(content = content))
        assertEquals("<p>core</p>", shown.html)
        assertNull(shown.text)
        assertEquals("<p>server</p>", CryptoRules.displayBody(body(), view()).html)
    }

    @Test fun aPassphrasePromptBelongsToTheShownPageOnly() {
        val locked = view(encryption = EncryptionState.NEED_PASSPHRASE)
        assertTrue(CryptoRules.mayPrompt(locked, settled = true))
        assertFalse(CryptoRules.mayPrompt(locked, settled = false))
        assertFalse(CryptoRules.mayPrompt(view(), settled = true))
        assertTrue(CryptoRules.mayPrompt(view(encryption = EncryptionState.WRONG_PASSPHRASE), settled = true))
    }
}
