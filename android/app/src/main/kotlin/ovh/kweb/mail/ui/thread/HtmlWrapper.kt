package ovh.kweb.mail.ui.thread

/** The document the mail's HTML is shown in. */
object HtmlWrapper {
    /** No scripts, no plug-ins, no forms, no `<base>`, no frames, no audio or video; the mail cannot loosen this by adding its own. */
    const val POLICY = "script-src 'none'; object-src 'none'; form-action 'none'; base-uri 'none'; frame-src 'none'; media-src 'none'"

    /**
     * [html] (as sent, unchanged) in a small page: a viewport so the text is readable, the
     * content security policy, and style that makes wide mail fit the screen and long words wrap.
     */
    fun wrap(html: String, textColor: String = "#000000", imageNote: String = "Image not loaded", blockRemote: Boolean = false, flat: Boolean = false): String = buildString {
        append("<!DOCTYPE html><html><head><meta charset=\"utf-8\">")
        // The page is as wide as the window and starts at scale 1; the user may pinch in on the mail (see MailWebView for how
        // that is kept away from the app's own layout). minimum-scale=1: no zooming out into an empty margin.
        append("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1, minimum-scale=1, maximum-scale=5, user-scalable=yes\">")
        append("<meta http-equiv=\"Content-Security-Policy\" content=\"").append(POLICY).append("\">")
        append("<meta http-equiv=\"x-dns-prefetch-control\" content=\"off\">")
        append("<style>")
        // The page is transparent: the card behind the view has the theme's background, and the text takes the theme's colour.
        append("html{background:transparent;color:").append(textColor).append("}")
        append("body{margin:0;padding:").append(if (flat) "12px 0" else "12px 14px").append(";word-wrap:break-word;overflow-wrap:anywhere}")
        // Only a remote picture that is waiting for a yes becomes a quiet dashed box (the img itself is replaced by the
        // note, so no broken-image glyph is left); a picture of the message itself (cid:) is left alone while it loads.
        if (blockRemote) {
            append("img[src^=\"http\"]{content:\"").append(imageNote.replace("\\", "").replace("\"", "")).append("\";display:block;box-sizing:border-box;border:1px dashed currentColor;border-radius:10px;padding:10px 12px;font:12px sans-serif;opacity:.7;width:auto;height:auto}")
        }
        // Mail must never need sideways panning: whatever is laid out for a wider screen is squeezed into the window.
        // Boxes and tables are held to the window's width (a max-width never widens anything, and an inline width of the mail
        // that is smaller still wins); cells may break long words and wrap, which is what lets an automatic-layout table shrink
        // below its widest word; pictures keep their proportions; preformatted text wraps.
        append("img{max-width:100%;height:auto}")
        append("table,div,center,section,article,blockquote,p,pre{max-width:100%}")
        append("td,th{overflow-wrap:anywhere;word-break:break-word;white-space:normal!important;min-width:0}")
        append("pre{white-space:pre-wrap;overflow-wrap:anywhere}")
        // Folded quoted text (QuoteFolding): a quiet pill that opens the quote; works without scripts.
        append("details.kq>summary{display:inline-block;cursor:pointer;list-style:none;font:12px sans-serif;opacity:.7;border:1px solid currentColor;border-radius:10px;padding:0 8px;margin:6px 0}")
        append("details.kq>summary::-webkit-details-marker{display:none}")
        append("</style></head><body>")
        append(stripViewport(html))
        append("</body></html>")
    }

    /** The mail's own viewport tag would override the one above (the last wins): fit and zoom are the app's to decide. */
    fun stripViewport(html: String): String = META_TAG.replace(html) { if (VIEWPORT_NAME.containsMatchIn(it.value)) "" else it.value }

    // The whole tag first (possessive, so a failed one stops at once), then the name inside it: linear on any input.
    private val META_TAG = Regex("""<meta\b[^<>]*+>""", RegexOption.IGNORE_CASE)
    private val VIEWPORT_NAME = Regex("""\bname\s*=\s*["']?viewport\b""", RegexOption.IGNORE_CASE)
}
