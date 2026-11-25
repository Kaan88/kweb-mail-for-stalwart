package ovh.kweb.mail.ui.thread

import kotlin.math.ceil
import kotlin.math.roundToInt

/**
 * When a mail is too wide for the window. The page is first laid out as wide as the window (`width=device-width`) and
 * overview mode scales it until its widest part fits. One wide part (a signature table of 550 pixels, a quoted Outlook
 * header, a long line) then made the whole mail small: its running text still wrapped at the window's width, but was
 * drawn at the scale of the wide part, at about 60 % on a phone, where Gmail shows it at full size (owner, 2026-11-24).
 *
 * Such a page is laid out a second time as wide as its content, with the view's text zoom raised by the factor the page
 * is scaled down ([textZoom]): the running text then fills the window's width at the size it would have in a page that
 * fits, while pictures and fixed layouts (newsletters) keep their shape. Nothing in the mail runs: no scripts, no new
 * permissions.
 */
object FitRules {
    /** Scaled less than this much (the page was at most 10 % wider than the window) is left as it is. */
    const val MIN_SHRINK = 1.1f

    /** A page more than this many times wider than the window (a very long unbroken line) is only scaled, as before. */
    const val MAX_SHRINK = 3.5f

    /**
     * The width (CSS pixels) to lay the page out at the second time, or null to leave it at the window's width.
     * [contentWidth] is how wide the page came out (CSS pixels), [windowWidth] how wide the view is (CSS pixels).
     */
    fun layoutWidth(contentWidth: Float, windowWidth: Float): Int? = fit(contentWidth, windowWidth)?.width

    /** The second layout: [width] in CSS pixels, and [shrink], how many times wider than the window it is. */
    data class Fit(val width: Int, val shrink: Float)

    /** As [layoutWidth], with the shrink factor. */
    fun fit(contentWidth: Float, windowWidth: Float): Fit? {
        if (contentWidth <= 0f || windowWidth <= 0f) return null
        val shrink = contentWidth / windowWidth
        if (shrink < MIN_SHRINK || shrink > MAX_SHRINK) return null
        val width = ceil(contentWidth).toInt()
        return Fit(width, width / windowWidth)
    }

    /**
     * The most the text is made larger because of the scaling. Above it, fixed-width layouts (cells of a set width, boxes
     * of a set height) would clip or overlap their text, so a very wide page keeps part of its scaling.
     */
    const val MAX_ZOOM_FACTOR = 2f

    /**
     * The WebView's text zoom (percent) for the user's [userZoom] in [fit]'s layout: the text is drawn at the size it
     * would have in a page that fits, up to [MAX_ZOOM_FACTOR].
     */
    fun textZoom(userZoom: Int, fit: Fit?): Int =
        if (fit == null) userZoom else (userZoom * minOf(fit.shrink, MAX_ZOOM_FACTOR)).roundToInt()
}
