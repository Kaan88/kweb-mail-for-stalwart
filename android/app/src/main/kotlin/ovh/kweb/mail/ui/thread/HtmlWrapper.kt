package ovh.kweb.mail.ui.thread

/** The document the mail's HTML is shown in. */
object HtmlWrapper {
    /** No scripts, no plug-ins, no forms, no `<base>`, no frames, no audio or video; the mail cannot loosen this by adding its own. */
    const val POLICY = "script-src 'none'; object-src 'none'; form-action 'none'; base-uri 'none'; frame-src 'none'; media-src 'none'"

    /**
     * [html] (as sent, unchanged) in a small page: a viewport so the text is readable, the
     * content security policy, and style that makes wide mail fit the screen and long words wrap.
     */
    fun wrap(
        html: String,
        textColor: String = "#000000",
        imageNote: String = "Image not loaded",
        blockRemote: Boolean = false,
        flat: Boolean = false,
        layoutWidth: Int? = null,
        /** CSS zoom for the outer tables (FitRules.tableZoom), or null. */
        tableZoom: Float? = null,
    ): String = buildString {
        append("<!DOCTYPE html><html><head><meta charset=\"utf-8\">")
        // Fit as Gmail does: the page is as wide as the window, a mail laid out for a wider one keeps its own layout and is
        // scaled down until it fits (overview mode in MailWebView), and the user may pinch in up to five times. No
        // initial-scale and no minimum-scale: either would stop the view from starting at the scale that fits. A mail
        // that came out wider is laid out again as wide as it is ([layoutWidth], see FitRules), so its text fills the window.
        append("<meta name=\"viewport\" content=\"").append(viewport(layoutWidth)).append("\">")
        append("<meta http-equiv=\"Content-Security-Policy\" content=\"").append(POLICY).append("\">")
        append("<meta http-equiv=\"x-dns-prefetch-control\" content=\"off\">")
        append("<style>")
        // The page is transparent: the card behind the view has the theme's background, and the text takes the theme's colour.
        append("html{background:transparent;color:").append(textColor).append("}")
        append("body{margin:0;padding:").append(if (flat) "12px 0" else "12px 14px").append(";overflow-wrap:break-word}")
        // Only a remote picture that is waiting for a yes becomes a quiet dashed box (the img itself is replaced by the
        // note, so no broken-image glyph is left); a picture of the message itself (cid:) is left alone while it loads.
        if (blockRemote) {
            append("img[src^=\"http\"]{content:\"").append(imageNote.replace("\\", "").replace("\"", "")).append("\";display:block;box-sizing:border-box;border:1px dashed currentColor;border-radius:10px;padding:10px 12px;font:12px sans-serif;opacity:.7;width:auto;height:auto}")
        }
        // Tables and boxes keep the widths the mail gave them (squeezing them broke the layout of newsletters); the scale
        // brings the whole page into the window instead. What has no width of its own is kept from widening the page:
        // pictures stay within their box with their proportions, and preformatted text and long words wrap.
        append("img{max-width:100%;height:auto}")
        append("pre{white-space:pre-wrap;overflow-wrap:break-word}")
        // The third layout of a wide mail (FitRules.refit): outer tables are drawn at the size they had before the text
        // zoom. Only outer ones (a nested table would be zoomed twice), and only those with no width of their own: zoom
        // scales a set width too, so a 600-pixel table would come out narrower than the window.
        if (tableZoom != null && tableZoom > 0f && tableZoom < 1f) {
            append("table:not(table table):not([width]):not([style*=\"width\" i]){zoom:").append(String.format(java.util.Locale.ROOT, "%.4f", tableZoom)).append("}")
        }
        // Folded quoted text (QuoteFolding): a quiet pill that opens the quote; works without scripts.
        append("details.kq>summary{display:inline-block;cursor:pointer;list-style:none;font:12px sans-serif;opacity:.7;border:1px solid currentColor;border-radius:10px;padding:0 8px;margin:6px 0}")
        append("details.kq>summary::-webkit-details-marker{display:none}")
        append("</style></head><body>")
        append(stripViewport(html))
        append("</body></html>")
    }

    /** The page's viewport: see [wrap]. */
    const val VIEWPORT = "width=device-width, maximum-scale=5, user-scalable=yes"

    /** [VIEWPORT], or the same with a width in CSS pixels (a whole number, so nothing else can get into the tag). */
    fun viewport(layoutWidth: Int?): String =
        if (layoutWidth == null || layoutWidth <= 0) VIEWPORT else "width=$layoutWidth, maximum-scale=5, user-scalable=yes"

    /** The mail's own viewport tag would override the one above (the last wins): fit and zoom are the app's to decide. */
    fun stripViewport(html: String): String = META_TAG.replace(html) { if (VIEWPORT_NAME.containsMatchIn(it.value)) "" else it.value }

    // The whole tag first (possessive, so a failed one stops at once), then the name inside it: linear on any input.
    private val META_TAG = Regex("""<meta\b[^<>]*+>""", RegexOption.IGNORE_CASE)
    private val VIEWPORT_NAME = Regex("""\bname\s*=\s*["']?viewport\b""", RegexOption.IGNORE_CASE)
}
