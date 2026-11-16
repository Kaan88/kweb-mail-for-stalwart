package ovh.kweb.mail.ui.crypto

import androidx.annotation.StringRes
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import ovh.kweb.mail.R
import uniffi.kweb_core.CryptoNote
import uniffi.kweb_core.CryptoView
import uniffi.kweb_core.EncryptionState
import uniffi.kweb_core.KeyKind
import uniffi.kweb_core.SignatureState

/** The words of the chip: one line. The signer's name is only said for a signature that is good news. */
@Composable
internal fun chipText(v: CryptoView): String {
    val name = v.signer?.label.orEmpty()
    if (v.encryption == EncryptionState.DECRYPTED) {
        return if (v.signature == SignatureState.VERIFIED) {
            stringResource(R.string.crypto_chip_decrypted_signed, name)
        } else {
            stringResource(R.string.crypto_chip_decrypted)
        }
    }
    val res = when (v.encryption) {
        EncryptionState.NEED_KEY -> R.string.crypto_chip_need_key
        EncryptionState.NEED_PASSPHRASE -> R.string.crypto_chip_need_passphrase
        EncryptionState.WRONG_PASSPHRASE -> R.string.crypto_chip_wrong_passphrase
        EncryptionState.NO_INTEGRITY -> R.string.crypto_chip_no_integrity
        EncryptionState.UNSUPPORTED -> R.string.crypto_chip_enc_unsupported
        EncryptionState.FAILED -> R.string.crypto_chip_enc_failed
        else -> null
    }
    if (res != null) return stringResource(res)
    return when (v.signature) {
        SignatureState.VERIFIED -> stringResource(R.string.crypto_chip_verified, name)
        SignatureState.KEY_CHANGED -> stringResource(R.string.crypto_chip_key_changed)
        SignatureState.FIRST_SEEN_KEY -> stringResource(R.string.crypto_chip_first_seen)
        SignatureState.CERTIFICATE_NOT_CHECKED -> stringResource(R.string.crypto_chip_cert_not_checked)
        SignatureState.OTHER_ADDRESS -> stringResource(R.string.crypto_chip_other_address)
        SignatureState.NO_KEY -> stringResource(R.string.crypto_chip_no_key)
        SignatureState.INVALID -> stringResource(R.string.crypto_chip_invalid)
        SignatureState.KEY_EXPIRED -> stringResource(R.string.crypto_chip_key_expired)
        SignatureState.KEY_REVOKED -> stringResource(R.string.crypto_chip_key_revoked)
        SignatureState.UNSUPPORTED -> stringResource(R.string.crypto_chip_unsupported)
        SignatureState.UNREADABLE -> stringResource(R.string.crypto_chip_unreadable)
        SignatureState.NOT_SIGNED -> ""
    }
}

@StringRes
internal fun whyRes(v: CryptoView): Int = when (v.encryption) {
    EncryptionState.DECRYPTED -> signatureWhy(v) ?: R.string.crypto_why_decrypted
    EncryptionState.NEED_KEY -> R.string.crypto_why_need_key
    EncryptionState.NEED_PASSPHRASE -> R.string.crypto_why_need_passphrase
    EncryptionState.WRONG_PASSPHRASE -> R.string.crypto_why_wrong_passphrase
    EncryptionState.NO_INTEGRITY -> R.string.crypto_why_no_integrity
    EncryptionState.UNSUPPORTED -> R.string.crypto_why_enc_unsupported
    EncryptionState.FAILED -> R.string.crypto_why_enc_failed
    EncryptionState.NOT_ENCRYPTED -> signatureWhy(v) ?: R.string.crypto_why_unreadable
}

@StringRes
private fun signatureWhy(v: CryptoView): Int? = when (v.signature) {
    SignatureState.NOT_SIGNED -> null
    SignatureState.VERIFIED -> R.string.crypto_why_verified
    SignatureState.KEY_CHANGED -> R.string.crypto_why_key_changed
    SignatureState.FIRST_SEEN_KEY -> R.string.crypto_why_first_seen
    SignatureState.CERTIFICATE_NOT_CHECKED -> R.string.crypto_why_cert_not_checked
    SignatureState.OTHER_ADDRESS -> R.string.crypto_why_other_address
    SignatureState.NO_KEY -> R.string.crypto_why_no_key
    SignatureState.INVALID -> R.string.crypto_why_invalid
    SignatureState.KEY_EXPIRED -> R.string.crypto_why_key_expired
    SignatureState.KEY_REVOKED -> R.string.crypto_why_key_revoked
    SignatureState.UNSUPPORTED -> R.string.crypto_why_unsupported
    SignatureState.UNREADABLE -> R.string.crypto_why_unreadable
}

@StringRes
private fun noteRes(n: CryptoNote): Int = when (n) {
    CryptoNote.ONLY_PART_COVERED -> R.string.crypto_part
    CryptoNote.REMOTE_CONTENT_BLOCKED -> R.string.crypto_note_remote_blocked
    CryptoNote.UNSIGNED_TEXT_HIDDEN -> R.string.crypto_note_unsigned_hidden
    CryptoNote.SIGNATURE_EXPIRED -> R.string.crypto_note_signature_expired
    CryptoNote.SIGNED_INSIDE_ENCRYPTION -> R.string.crypto_note_signed_inside
    CryptoNote.NOT_SEARCHABLE -> R.string.crypto_note_not_searchable
    CryptoNote.SMIME_NO_REVOCATION_CHECK -> R.string.crypto_note_smime_no_revocation
}

/** The status of one message in one line, under the sender. A tap opens the explanation. */
@Composable
fun CryptoChip(ui: CryptoUi, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val colors = MaterialTheme.colorScheme
    val tone = (ui as? CryptoUi.Ready)?.let { CryptoRules.tone(it.view) }
    val (container, content) = when (tone) {
        CryptoRules.Tone.GOOD -> colors.primaryContainer to colors.onPrimaryContainer
        CryptoRules.Tone.WARNING -> colors.tertiaryContainer to colors.onTertiaryContainer
        CryptoRules.Tone.BAD -> colors.errorContainer to colors.onErrorContainer
        else -> colors.surfaceContainerHighest to colors.onSurfaceVariant
    }
    Row(
        modifier
            .clip(RoundedCornerShape(50))
            .background(container)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        when (ui) {
            CryptoUi.Checking -> CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp, color = content)
            else -> Icon(iconFor(tone, ui), contentDescription = null, modifier = Modifier.size(16.dp), tint = content)
        }
        val text = when (ui) {
            CryptoUi.Checking -> stringResource(R.string.crypto_chip_checking)
            CryptoUi.Failed -> stringResource(R.string.crypto_chip_failed)
            is CryptoUi.Ready -> chipText(ui.view)
        }
        Text(text, color = content, style = MaterialTheme.typography.labelLarge, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

private fun iconFor(tone: CryptoRules.Tone?, ui: CryptoUi): ImageVector = when {
    tone == CryptoRules.Tone.GOOD -> Icons.Filled.Check
    tone == CryptoRules.Tone.WARNING || tone == CryptoRules.Tone.BAD || ui == CryptoUi.Failed -> Icons.Filled.Warning
    else -> Icons.Filled.Lock
}

/** The plain-words explanation behind the chip, what the check covers, and the keys the message carries. */
@Composable
fun CryptoDialog(
    ui: CryptoUi,
    onDismiss: () -> Unit,
    onRetry: () -> Unit,
    onTakeOffer: (String) -> Unit,
    onPassphrase: (() -> Unit)? = null,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.crypto_why_title)) },
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.crypto_close)) } },
        dismissButton = when {
            ui == CryptoUi.Failed -> ({ TextButton(onClick = onRetry) { Text(stringResource(R.string.crypto_retry)) } })
            ui is CryptoUi.Ready && onPassphrase != null && CryptoRules.needsPassphrase(ui.view) ->
                ({ TextButton(onClick = onPassphrase) { Text(stringResource(R.string.crypto_enter_passphrase)) } })
            else -> null
        },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                when (ui) {
                    CryptoUi.Checking -> Text(stringResource(R.string.crypto_chip_checking))
                    CryptoUi.Failed -> Text(stringResource(R.string.crypto_chip_failed))
                    is CryptoUi.Ready -> ReadyBody(ui.view, onTakeOffer)
                }
            }
        },
    )
}

@Composable
private fun ReadyBody(v: CryptoView, onTakeOffer: (String) -> Unit) {
    val detail = v.detail ?: stringResource(R.string.crypto_why_no_detail)
    Text(stringResource(whyRes(v), detail), style = MaterialTheme.typography.bodyMedium)
    if (v.from.isNotBlank()) {
        Text(stringResource(R.string.crypto_from_line, v.from), style = MaterialTheme.typography.bodyMedium)
    }
    v.signer?.let { s ->
        Text(stringResource(R.string.crypto_signer, s.label), style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
        if (s.emails.isNotEmpty()) Text(stringResource(R.string.crypto_signer_addresses, s.emails.joinToString(", ")), style = MaterialTheme.typography.bodySmall)
        Text(stringResource(R.string.crypto_fingerprint, s.fingerprint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(stringResource(if (s.inKeyring) R.string.crypto_in_keyring else R.string.crypto_not_in_keyring), style = MaterialTheme.typography.bodySmall)
    }
    if (v.signature != SignatureState.NOT_SIGNED && v.coversWholeMessage) {
        Text(stringResource(R.string.crypto_whole), style = MaterialTheme.typography.bodySmall)
    }
    v.notes.forEach { Text(stringResource(noteRes(it)), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
    if (v.offers.isNotEmpty()) {
        Text(stringResource(R.string.crypto_offer_title), style = MaterialTheme.typography.titleSmall)
        v.offers.forEach { offer ->
            val kind = stringResource(if (offer.kind == KeyKind.PGP) R.string.crypto_kind_pgp else R.string.crypto_kind_smime)
            Column {
                Text(stringResource(R.string.crypto_offer_line, offer.label, kind), style = MaterialTheme.typography.bodyMedium)
                Text(offer.fingerprint, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                if (offer.alreadyKnown) {
                    Text(stringResource(R.string.crypto_offer_known), style = MaterialTheme.typography.labelMedium)
                } else {
                    TextButton(onClick = { onTakeOffer(offer.id) }, modifier = Modifier.padding(0.dp)) { Text(stringResource(R.string.crypto_offer_add)) }
                }
            }
        }
    }
}
