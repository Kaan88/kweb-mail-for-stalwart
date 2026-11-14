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
        append("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">")
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
        append("img,table{max-width:100%;height:auto}")
        append("pre{white-space:pre-wrap}")
        append("</style></head><body>")
        append(html)
        append("</body></html>")
    }
}
