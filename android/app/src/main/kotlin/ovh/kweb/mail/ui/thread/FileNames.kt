package ovh.kweb.mail.ui.thread

import java.security.MessageDigest

/** Names for files the app writes under its cache. Nothing from a mail reaches a path unchecked. */
object FileNames {
    /**
     * What a downloaded blob's cache name is made from: the blob id alone for the user's own mail (as it always
     * was), the account and the blob id for a shared account. A blob id is only unique within one account, so
     * two accounts' blobs must never share a file. The length prefix keeps "a" + "bc" apart from "ab" + "c".
     */
    fun blobKey(account: String?, blobId: String): String = if (account == null) blobId else "shared:${account.length}:$account:$blobId"

    /** The file system allows 255 bytes; the core adds a temporary suffix while it downloads. */
    private const val MAX_BYTES = 180

    /**
     * A file name a mail supplied, made safe to use in a path: path separators become "_",
     * control characters go, leading and trailing dots and blanks go (so no "..", no hidden
     * file), and a name over 180 bytes in UTF-8 is shortened by whole characters but keeps its
     * extension. Never empty.
     */
    fun safe(name: String, fallback: String = "attachment"): String {
        val cleaned = name
            .filterNot { Character.isISOControl(it) }
            .replace('/', '_')
            .replace('\\', '_')
            .trim { it == '.' || it.isWhitespace() }
        if (cleaned.isEmpty()) return fallback
        if (utf8Length(cleaned) <= MAX_BYTES) return cleaned
        val dot = cleaned.lastIndexOf('.')
        val extension = if (dot > 0 && cleaned.length - dot <= 16) cleaned.substring(dot) else ""
        val base = StringBuilder(cleaned.substring(0, cleaned.length - extension.length))
        // Cut whole characters (not half of a surrogate pair) until it fits.
        while (base.isNotEmpty() && utf8Length(base) + utf8Length(extension) > MAX_BYTES) {
            base.setLength(base.offsetByCodePoints(base.length, -1))
        }
        val shortened = base.toString().trimEnd { it == '.' || it.isWhitespace() }
        return if (shortened.isEmpty()) fallback else shortened + extension
    }

    private fun utf8Length(text: CharSequence): Int = text.toString().toByteArray(Charsets.UTF_8).size

    /**
     * The kind of a file as its tile shows it: the extension in capitals ("PDF"), when the name
     * has a short one made of letters and digits. Null otherwise.
     */
    fun kind(name: String): String? {
        val extension = name.substringAfterLast('.', "")
        val usable = extension.length in 1..4 && extension.all { it.isLetterOrDigit() } && name.length > extension.length + 1
        return if (usable) extension.uppercase() else null
    }

    /** Lower-case hex SHA-256 of [text]: the name of a blob in the cache, whatever characters its id has. */
    fun sha256Hex(text: String): String =
        MessageDigest.getInstance("SHA-256").digest(text.toByteArray(Charsets.UTF_8)).joinToString("") { "%02x".format(it) }
}
