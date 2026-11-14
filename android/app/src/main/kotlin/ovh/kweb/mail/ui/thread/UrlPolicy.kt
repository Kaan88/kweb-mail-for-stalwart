package ovh.kweb.mail.ui.thread

import java.io.ByteArrayOutputStream

/** What the mail view does with each address the mail refers to. No Android classes, so it is tested on the JVM. */
object UrlPolicy {
    /** What to answer a request the page makes. */
    sealed interface Fetch {
        /** `cid:`: the inline attachment with this Content-ID. */
        data class Inline(val contentId: String) : Fetch

        /** Let the WebView load it: remote content, and the user allowed it. */
        data object Load : Fetch

        /** Answer with nothing. [remote] says the mail wanted something from the internet, so the screen offers to show it. */
        data class Empty(val remote: Boolean) : Fetch
    }

    /** What a tap on a link does. */
    sealed interface Link {
        /** Hand it to another app. */
        data class Open(val url: String) : Link

        data object Ignore : Link
    }

    /** A sub-resource of the page (picture, style sheet, frame, font…). */
    fun fetch(url: String, allowRemote: Boolean): Fetch = when (scheme(url)) {
        "cid" -> Fetch.Inline(percentDecode(url.substring("cid:".length)).trim().removeSurrounding("<", ">"))
        "http", "https" -> if (allowRemote) Fetch.Load else Fetch.Empty(remote = true)
        else -> Fetch.Empty(remote = false)
    }

    /** A link the user tapped. The view itself never navigates. */
    fun link(url: String): Link = when (scheme(url)) {
        "http", "https", "mailto" -> Link.Open(url)
        else -> Link.Ignore
    }

    /** A `mailto:` link goes to a mail app (`ACTION_SENDTO`), the others to a browser. */
    fun isMail(url: String): Boolean = scheme(url) == "mailto"

    private fun scheme(url: String): String? {
        val end = url.indexOf(':')
        if (end <= 0) return null
        val scheme = url.substring(0, end)
        // Only letters, digits, "+", "-" and "." make a scheme; anything else is no scheme at all.
        return if (scheme.all { it.isLetterOrDigit() || it == '+' || it == '-' || it == '.' }) scheme.lowercase() else null
    }

    /** "%C5%9F" is "ş". A "%" not followed by two hex digits stays as it is; "+" stays a plus. */
    internal fun percentDecode(text: String): String {
        if ('%' !in text) return text
        val out = ByteArrayOutputStream()
        var i = 0
        while (i < text.length) {
            val high = if (text[i] == '%' && i + 2 < text.length) Character.digit(text[i + 1], 16) else -1
            val low = if (high >= 0) Character.digit(text[i + 2], 16) else -1
            if (low >= 0) {
                out.write(high * 16 + low)
                i += 3
            } else {
                val codePoint = text.codePointAt(i)
                out.write(String(Character.toChars(codePoint)).toByteArray(Charsets.UTF_8))
                i += Character.charCount(codePoint)
            }
        }
        return String(out.toByteArray(), Charsets.UTF_8)
    }
}
