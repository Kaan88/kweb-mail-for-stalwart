package ovh.kweb.mail.ui.thread

/**
 * When the page of a [MailWebView] may be shown, as plain steps so the order of the WebView's callbacks can be tested.
 * Each load is numbered ([newPage]); a callback that belongs to an earlier number is ignored. A page that is to be
 * measured is measured on every finish of its load (a load may finish twice, and a late finish of a replaced load counts
 * for the new one), and once a measurement said the page will be laid out again, that number is never shown: the next
 * load is. A page that never reports is shown by [timedOut].
 */
internal class PageReveal(private val show: () -> Unit, private val hide: () -> Unit) {
    private var page = 0
    private var measure = false

    /** The number of the load whose measurement asked for a second layout; 0 for none. */
    private var refitting = 0

    /** A new document is about to be loaded; [measure]: report its width once drawn. Returns its number. */
    fun newPage(measure: Boolean): Int {
        this.measure = measure
        return ++page
    }

    /** The number of the current load, taken when a load finished and handed to [drawn] once the page was drawn. */
    fun finished(): Int = page

    /**
     * The load [number] finished and was drawn. [loadsAgain] measures it and answers whether it will be loaded again at
     * its own width; it is only asked while the page is to be measured.
     */
    fun drawn(number: Int, loadsAgain: () -> Boolean) {
        if (number != page || refitting == number) return
        if (measure && loadsAgain()) {
            refitting = number
            // Whether or not it was shown yet (the fallback, a view made again after a crash): hidden until the next is drawn.
            hide()
            return
        }
        show()
    }

    /** The time for load [number] to report ran out: a renderer that never reports must not leave the mail hidden. */
    fun timedOut(number: Int) {
        if (number == page) show()
    }
}
