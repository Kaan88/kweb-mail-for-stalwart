package ovh.kweb.mail.ui.thread

/** The document the mail's HTML is shown in. */
object HtmlWrapper {
    /** No scripts, no plug-ins, no forms, no `<base>`, no frames, no audio or video; the mail cannot loosen this by adding its own. */
    const val POLICY = "script-src 'none'; object-src 'none'; form-action 'none'; base-uri 'none'; frame-src 'none'; media-src 'none'"

    /**
     * [html] (as sent, unchanged) in a small page: a viewport so the text is readable, the
     * content security policy, and style that makes wide mail fit the screen and long words wrap.
     */
    fun wrap(html: String): String = buildString {
        append("<!DOCTYPE html><html><head><meta charset=\"utf-8\">")
        append("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">")
        append("<meta http-equiv=\"Content-Security-Policy\" content=\"").append(POLICY).append("\">")
        append("<meta http-equiv=\"x-dns-prefetch-control\" content=\"off\">")
        append("<style>")
        append("html{background:#fff;color:#000}")
        append("body{margin:0;padding:0;word-wrap:break-word;overflow-wrap:anywhere}")
        append("img,table{max-width:100%;height:auto}")
        append("pre{white-space:pre-wrap}")
        append("</style></head><body>")
        append(html)
        append("</body></html>")
    }
}
