package ovh.kweb.mail.ui.files

import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale
import uniffi.kweb_core.FileEntry
import uniffi.kweb_core.FileKind

/** The picture on a row. */
enum class FileIcon { Folder, Picture, Text, Pdf, Other }

/** What a row's menu can offer. */
enum class RowAction { Open, Save, Rename, Move, Delete }

/** The answer to "that name is already there". */
enum class ConflictChoice { Replace, KeepBoth, Skip }

/** The decisions of the Files screens that need no Android: wording of sizes and dates, icons, names, menus. */
/** The words after a size; the screens take them from the string resources. */
data class SizeUnits(val byte: String = "B", val kilo: String = "KB", val mega: String = "MB", val giga: String = "GB", val tera: String = "TB")

object FileRules {
    /**
     * A size as people read it, in thousands as Android's own file sizes are: "0 B", "999 B", "1.5 KB",
     * "25 MB". One decimal below 10, none from 10 up.
     */
    fun sizeText(bytes: ULong, locale: Locale, words: SizeUnits = SizeUnits()): String {
        if (bytes < 1000uL) return "$bytes ${words.byte}"
        val units = listOf(words.kilo, words.mega, words.giga, words.tera)
        var value = bytes.toDouble() / 1000.0
        var unit = 0
        while (value >= 999.95 && unit < units.size - 1) {
            value /= 1000.0
            unit++
        }
        val number = if (value < 9.95) String.format(locale, "%.1f", value) else String.format(locale, "%.0f", value)
        return "$number ${units[unit]}"
    }

    /** The day of [epochSeconds] in [zone], as the locale writes a medium date ("14 Nov 2026"). */
    fun dateText(epochSeconds: Long, zone: ZoneId, locale: Locale): String =
        DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale).format(Instant.ofEpochSecond(epochSeconds).atZone(zone))

    /** The quiet line under a name: size and date of a file ("1.5 KB · 14 Nov 2026"), nothing for a folder. */
    fun subtitle(entry: FileEntry, zone: ZoneId, locale: Locale, words: SizeUnits = SizeUnits()): String {
        if (entry.kind != FileKind.FILE) return ""
        val parts = listOfNotNull(sizeText(entry.size, locale, words), entry.modified?.let { dateText(it, zone, locale) })
        return parts.joinToString(" · ")
    }

    private val pictureExtensions = setOf("png", "jpg", "jpeg", "gif", "webp", "bmp", "heic", "heif", "avif", "svg")
    private val textExtensions = setOf("txt", "md", "csv", "log", "json", "xml", "yml", "yaml", "html", "htm", "ini", "conf", "rtf")
    private val textTypes = setOf("application/json", "application/xml", "application/x-yaml", "application/yaml", "application/rtf")

    /** The icon of an entry, from its kind and media type; a file the server calls plain binary is judged by its extension. */
    fun iconFor(kind: FileKind, mediaType: String, name: String): FileIcon {
        when (kind) {
            FileKind.FOLDER -> return FileIcon.Folder
            FileKind.OTHER -> return FileIcon.Other
            FileKind.FILE -> Unit
        }
        val type = mediaType.substringBefore(';').trim().lowercase()
        when {
            type.startsWith("image/") -> return FileIcon.Picture
            type == "application/pdf" -> return FileIcon.Pdf
            type.startsWith("text/") || type in textTypes || type.endsWith("+json") || type.endsWith("+xml") -> return FileIcon.Text
        }
        if (type.isEmpty() || type == "application/octet-stream") {
            val extension = name.substringAfterLast('.', "").lowercase()
            return when {
                extension in pictureExtensions -> FileIcon.Picture
                extension == "pdf" -> FileIcon.Pdf
                extension in textExtensions -> FileIcon.Text
                else -> FileIcon.Other
            }
        }
        return FileIcon.Other
    }

    private val reservedNames = Regex("(?i)(CON|PRN|AUX|NUL|COM[0-9]|LPT[0-9])")
    private const val SERVER_NAME_BYTES = 255
    private const val FORBIDDEN = "/<>:\\\"|?*"

    /**
     * The name a picked document gets on the server: what the provider calls it, changed only where the server
     * would refuse it (notes: `/ < > : " \ | ? *` and control characters, "." and "..", the reserved device names,
     * more than 255 bytes). Leading dots and blanks stay. [fallback] when there is no name.
     */
    fun serverName(raw: String?, fallback: String = "attachment"): String {
        var name = (raw ?: "").map { if (Character.isISOControl(it) || it in FORBIDDEN) '_' else it }.joinToString("")
        if (name.isEmpty() || name == "." || name == "..") return fallback
        if (reservedNames.matches(name.substringBefore('.'))) name = "_$name"
        if (name.toByteArray(Charsets.UTF_8).size <= SERVER_NAME_BYTES) return name
        val dot = name.lastIndexOf('.')
        val extension = if (dot > 0 && name.length - dot <= 16) name.substring(dot) else ""
        val base = StringBuilder(name.substring(0, name.length - extension.length))
        while (base.isNotEmpty() && base.toString().toByteArray(Charsets.UTF_8).size + extension.toByteArray(Charsets.UTF_8).size > SERVER_NAME_BYTES) {
            base.setLength(base.offsetByCodePoints(base.length, -1))
        }
        return if (base.isEmpty()) fallback else base.toString() + extension
    }

    private val extensionTypes = mapOf(
        "pdf" to "application/pdf", "png" to "image/png", "jpg" to "image/jpeg", "jpeg" to "image/jpeg", "gif" to "image/gif",
        "webp" to "image/webp", "bmp" to "image/bmp", "heic" to "image/heic", "heif" to "image/heif", "avif" to "image/avif",
        "svg" to "image/svg+xml", "txt" to "text/plain", "md" to "text/markdown", "csv" to "text/csv", "log" to "text/plain",
        "json" to "application/json", "xml" to "application/xml", "html" to "text/html", "htm" to "text/html",
        "mp3" to "audio/mpeg", "wav" to "audio/wav", "ogg" to "audio/ogg", "mp4" to "video/mp4", "webm" to "video/webm",
        "zip" to "application/zip", "doc" to "application/msword",
        "docx" to "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" to "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" to "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" to "application/vnd.oasis.opendocument.text", "ods" to "application/vnd.oasis.opendocument.spreadsheet",
    )

    /**
     * The type to hand to a viewer: the stored one, but when that is blank or plain binary (what the server says
     * when the creator gave none) the one the extension suggests, as the icon does. Still plain binary when unknown.
     */
    fun viewType(mediaType: String, name: String): String {
        val type = mediaType.substringBefore(';').trim().lowercase()
        if (type.isNotEmpty() && type != "application/octet-stream") return mediaType
        val extension = name.substringAfterLast('.', "").lowercase()
        return extensionTypes[extension] ?: "application/octet-stream"
    }

    /** Where the extension starts in [name] (a leading dot is part of the name, not an extension); the length when there is none. */
    private fun extensionStart(name: String): Int {
        val dot = name.lastIndexOf('.')
        return if (dot > 0) dot else name.length
    }

    /**
     * Where the selection in the rename dialog ends: up to the extension of a file, so typing replaces the name
     * and keeps ".pdf"; the whole name for a folder or a name without an extension.
     */
    fun renameSelectionEnd(name: String, kind: FileKind): Int = if (kind == FileKind.FILE) extensionStart(name) else name.length

    /**
     * A name for "Keep both": [name] itself when it is free, else "name (2).ext", "name (3).ext" and so on until
     * the name is not in [taken]. The server tells names apart by case, so [taken] is compared as it is.
     */
    fun freeName(name: String, taken: Collection<String>): String {
        if (name !in taken) return name
        val cut = extensionStart(name)
        val base = name.substring(0, cut)
        val extension = name.substring(cut)
        var n = 2
        while (true) {
            val candidate = "$base ($n)$extension"
            if (candidate !in taken) return candidate
            n++
        }
    }

    /** The folders the move dialog lists: the folders of the listing, but never the item being moved. */
    fun moveChoices(entries: List<FileEntry>, moving: FileEntry): List<FileEntry> =
        entries.filter { it.kind == FileKind.FOLDER && it.id != moving.id }

    /**
     * Whether "Move here" is on for the folder the dialog shows ([trail] is the way down to it, empty at the top):
     * not into the item itself or a folder in it, not where it already is, and only where the user may add.
     * [mayAdd] is the shown listing's own answer.
     */
    fun canMoveHere(moving: FileEntry, trail: List<FileEntry>, mayAdd: Boolean): Boolean =
        mayAdd && trail.none { it.id == moving.id } && trail.lastOrNull()?.id != moving.parentId

    /** What the menu of a row offers, in order, for the rights of the entry. */
    fun rowActions(entry: FileEntry): List<RowAction> = buildList {
        when (entry.kind) {
            FileKind.FOLDER -> add(RowAction.Open)
            FileKind.FILE -> {
                add(RowAction.Open)
                add(RowAction.Save)
            }
            FileKind.OTHER -> Unit
        }
        if (entry.kind != FileKind.OTHER && entry.mayRename) {
            add(RowAction.Rename)
            add(RowAction.Move)
        }
        if (entry.mayDelete) add(RowAction.Delete)
    }

    /** True when a file of [size] bytes is more than the server takes. A limit of 0 means the server did not say. */
    fun tooLarge(size: Long, maxUpload: ULong): Boolean = maxUpload > 0uL && size >= 0 && size.toULong() > maxUpload

    /** The line under the top bar: the space and the folders down to the one shown. */
    fun pathLine(spaceName: String, trail: List<FileEntry>): String = (listOf(spaceName) + trail.map { it.name }).joinToString(" / ")

    /** What the user may do with the name they typed: it counts when it is not blank. */
    fun cleanName(text: String): String = text.trim()
}
