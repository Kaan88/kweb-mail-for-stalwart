package ovh.kweb.mail.ui.crypto

import android.content.Context
import android.content.Intent
import android.util.Log
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import java.io.File
import java.util.Locale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountPaths
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.thread.FileNames
import ovh.kweb.mail.ui.thread.SizeFormat
import uniffi.kweb_core.CryptoAttachment

/** Asks for the passphrase of a key. The text is held in this dialog only and goes to the core with the tap. */
@Composable
fun PassphraseDialog(
    keyName: String,
    wrong: Boolean,
    onSubmit: (String, Boolean) -> Unit,
    onDismiss: () -> Unit,
    showKeep: Boolean = true,
    body: String? = null,
    confirmLabel: Int = R.string.crypto_passphrase_open,
) {
    var pass by remember { mutableStateOf("") }
    var keep by remember { mutableStateOf(false) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.crypto_passphrase_title)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(body ?: stringResource(R.string.crypto_passphrase_body, keyName))
                if (wrong) Text(stringResource(R.string.crypto_wrong_passphrase), color = MaterialTheme.colorScheme.error)
                OutlinedTextField(
                    value = pass,
                    onValueChange = { pass = it },
                    label = { Text(stringResource(R.string.crypto_passphrase_hint)) },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    modifier = Modifier.fillMaxWidth(),
                )
                if (showKeep) {
                    Row(Modifier.fillMaxWidth().clickable { keep = !keep }, verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(checked = keep, onCheckedChange = { keep = it })
                        Column {
                            Text(stringResource(R.string.crypto_passphrase_keep))
                            if (keep) {
                                Text(
                                    stringResource(R.string.crypto_unlock_keep_detail),
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                        }
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = { onSubmit(pass, keep) }) { Text(stringResource(confirmLabel)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

/** The files of an opened encrypted message, one group. A tap opens one; it is written to a folder that is emptied when the screen is left. */
@Composable
fun CryptoFiles(files: List<CryptoAttachment>, flat: Boolean, onOpen: (CryptoAttachment) -> Unit) {
    Column(Modifier.padding(top = 8.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
        Text(stringResource(R.string.crypto_attachments), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
        files.forEachIndexed { index, file ->
            val size = remember(file.size) { SizeFormat.format(file.size.toLong(), Locale.getDefault()) }
            Column(
                Modifier
                    .fillMaxWidth()
                    .clip(Groups.shape(index, files.size))
                    .background(if (flat) MaterialTheme.colorScheme.surfaceContainerHigh else MaterialTheme.colorScheme.background)
                    .clickable { onOpen(file) }
                    .padding(horizontal = 14.dp, vertical = 10.dp),
            ) {
                Text(file.name.ifBlank { stringResource(R.string.thread_attachment) }, maxLines = 1)
                Text(size, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        Text(stringResource(R.string.crypto_attachment_note), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/**
 * Where the files of an opened encrypted message go when the user opens one: `cache/attachments/<account>/crypto/`,
 * inside what the FileProvider shares and under the account's own attachment folder (so removing the account removes it).
 * The folder is emptied when the thread screen is left and at every start of the app; nothing there is kept.
 */
object CryptoTemp {
    private fun dir(context: Context, accountId: String) = File(context.cacheDir, AccountPaths.attachments(accountId) + "/crypto")

    fun clear(context: Context, accountId: String) {
        dir(context, accountId).deleteRecursively()
    }

    /** Writes the file out of the core's memory and builds the intent that opens it; null when that fails. Off the main thread. */
    suspend fun open(context: Context, session: AccountSession, messageId: String, file: CryptoAttachment): Intent? =
        withContext(Dispatchers.IO) {
            try {
                val folder = File(dir(context, session.record.id), "${FileNames.sha256Hex(messageId)}-${file.index}")
                folder.mkdirs()
                val target = File(folder, FileNames.safe(file.name.ifBlank { "file" }))
                session.core.cryptoSaveAttachment(messageId, file.index.toUInt(), target.absolutePath)
                val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", target)
                Intent(Intent.ACTION_VIEW)
                    .setDataAndType(uri, file.mimeType.ifBlank { "application/octet-stream" })
                    .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            } catch (e: Exception) {
                // The class only: a message of the core may name the file.
                Log.w(Background.TAG, "opening a decrypted file failed: ${e.javaClass.simpleName}")
                null
            }
        }
}
