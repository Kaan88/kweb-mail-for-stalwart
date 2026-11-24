package ovh.kweb.mail.ui.thread

import android.os.Build
import android.view.MotionEvent
import android.view.ViewConfiguration
import android.view.ViewGroup
import android.content.Context
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.rememberScrollableState
import androidx.compose.foundation.gestures.scrollable

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.Velocity
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import java.io.ByteArrayInputStream
import kotlin.math.abs
import kotlin.math.roundToInt
import java.io.InputStream
import ovh.kweb.mail.R

/**
 * What the page behind the WebView may reach. Read on WebView threads, written on the main
 * thread, hence the volatile fields.
 */
private class PageRules(
    /** The bytes and type of the inline picture with this Content-ID, or null. Blocks. */
    @Volatile var inline: (contentId: String) -> Pair<String, InputStream>?,
    @Volatile var allowRemote: Boolean,
    @Volatile var onBlocked: Runnable,
    @Volatile var openLink: (String) -> Unit,
    @Volatile var onGone: Runnable,
)

/**
 * The HTML of a mail, shown with everything locked down: no JavaScript, no file or content
 * access, no storage. Every request the page makes goes through [UrlPolicy]: `cid:` pictures
 * come from [inline], remote content only when [allowRemote], everything else gets an empty
 * answer. The view never navigates; a tapped link is handed to [openLink].
 *
 * [onBlocked] is called (on the main thread) when remote content was refused. When the
 * WebView's renderer dies the view is made again once; if that one dies too, a note is shown.
 */
@Composable
fun MailWebView(
    html: String,
    allowRemote: Boolean,
    inline: (contentId: String) -> Pair<String, InputStream>?,
    onBlocked: () -> Unit,
    openLink: (String) -> Unit,
    modifier: Modifier = Modifier,
    /** Drawn straight on the page: no side padding in the document. */
    flat: Boolean = false,
    /** The user's text size for message bodies, in percent. */
    textZoom: Int = 100,
) {
    val dark = ovh.kweb.mail.ui.theme.LocalDarkTheme.current
    val textColor = MaterialTheme.colorScheme.onBackground.toArgb().let { String.format("#%06x", it and 0xFFFFFF) }
    val imageNote = stringResource(R.string.thread_image_not_loaded)
    var generation by remember { mutableIntStateOf(0) }
    var dead by remember { mutableStateOf(false) }
    val rules = remember { PageRules(inline, allowRemote, Runnable(onBlocked), openLink, Runnable { }) }
    SideEffect {
        rules.inline = inline
        rules.allowRemote = allowRemote
        rules.onBlocked = Runnable(onBlocked)
        rules.openLink = openLink
        rules.onGone = Runnable { if (generation == 0) generation = 1 else dead = true }
    }
    if (dead) {
        Text(
            stringResource(R.string.thread_cannot_display),
            modifier = modifier.fillMaxWidth().padding(vertical = 8.dp),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
        )
        return
    }

    key(generation) {
        // A page wider than the view (or zoomed) is moved here, in Compose, and not by the WebView's own touch handling: what
        // the page cannot move any further goes on to the parent as leftover, which is how the pager of the thread screen
        // takes a swipe at the edge of the page (and anywhere on a page that fits), and the thread's column a vertical drag
        // at the top or bottom of a zoomed mail.
        val panView = remember { arrayOfNulls<PanWebView>(1) }
        val pan = rememberScrollableState { delta -> panView[0]?.pan(-delta)?.let { -it } ?: 0f }
        val panDown = rememberScrollableState { delta -> panView[0]?.panDown(-delta)?.let { -it } ?: 0f }
        // Only a direction the page can move in takes part: over a page that fits, the pager and the column get the finger
        // directly, with their own fling and snap, and not what a scrollable in between hands on.
        var wide by remember { mutableStateOf(false) }
        var tall by remember { mutableStateOf(false) }
        // A drag that moved the page stops at its edge: what is left of it does not reach the pager. Only a drag that starts
        // at the edge (towards it) goes on to the next or previous conversation (owner, 2026-11-24).
        val edge = remember {
            object : NestedScrollConnection {
                override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset =
                    if (available.x == 0f || panView[0]?.mayHandOver(available.x) != false) Offset.Zero else Offset(available.x, 0f)

                override suspend fun onPostFling(consumed: Velocity, available: Velocity): Velocity =
                    if (available.x == 0f || panView[0]?.mayHandOver(available.x) != false) Velocity.Zero else Velocity(available.x, 0f)
            }
        }
        AndroidView(
            modifier = modifier
                .fillMaxWidth()
                .nestedScroll(edge)
                .scrollable(pan, Orientation.Horizontal, enabled = wide)
                .scrollable(panDown, Orientation.Vertical, enabled = tall),
            factory = { context ->
                PanWebView(context).apply {
                    panView[0] = this
                    onWide = { wide = it }
                    onTall = { tall = it }
                    layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
                    settings.javaScriptEnabled = false
                    settings.allowFileAccess = false
                    settings.allowContentAccess = false
                    settings.setGeolocationEnabled(false)
                    settings.domStorageEnabled = false
                    @Suppress("DEPRECATION")
                    settings.databaseEnabled = false
                    settings.setSupportMultipleWindows(false)
                    // Fit and zoom, as Gmail has it. useWideViewPort makes the view honour the page's viewport (as wide as the
                    // window, see HtmlWrapper); overview mode starts it at the smallest scale, which is the one where the
                    // mail's own layout fits the window: a newsletter made for 640 pixels is shown whole and smaller, a mail
                    // that fits already stays at scale 1. The user may pinch in on the mail: only this view scales, never
                    // the app's layout or the pager around it. The view keeps the height of the fitted mail while it is
                    // zoomed (see [PanWebView]), so the zoomed mail moves inside that frame, both ways, and hands a drag at
                    // its edge on to the pager or the column. The on-screen zoom buttons stay off.
                    settings.useWideViewPort = true
                    settings.loadWithOverviewMode = true
                    settings.setSupportZoom(true)
                    settings.builtInZoomControls = true
                    settings.displayZoomControls = false
                    // Safe Browsing is left at its default, which is on.
                    isVerticalScrollBarEnabled = false
                    overScrollMode = android.view.View.OVER_SCROLL_NEVER
                    setBackgroundColor(android.graphics.Color.TRANSPARENT)
                    webViewClient = RulesClient(rules)
                }
            },
            update = { view ->
                // Below Android 13 the page stays light, which is readable on any theme.
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) view.settings.isAlgorithmicDarkeningAllowed = dark
                // A second lock behind the request filter: the network is off unless the user said yes.
                view.settings.blockNetworkLoads = !allowRemote
                view.settings.textZoom = textZoom
                val shown = listOf(html, allowRemote, textColor, flat)
                if (view.tag != shown) {
                    view.tag = shown
                    view.newPage()
                    view.loadDataWithBaseURL(null, HtmlWrapper.wrap(html, textColor, imageNote, blockRemote = !allowRemote, flat = flat), "text/html", "utf-8", null)
                }
            },
            onRelease = { view ->
                panView[0] = null
                view.stopLoading()
                (view.parent as? ViewGroup)?.removeView(view)
                view.destroy()
            },
        )
    }
}

/** A WebView that can be moved sideways from outside; how far is its own business (its range, its zoom). */
private class PanWebView(context: Context) : WebView(context) {
    /** How far the page can still be moved to the right. */
    private fun maxScrollX(): Int = (computeHorizontalScrollRange() - computeHorizontalScrollExtent()).coerceAtLeast(0)

    /** How far the page can still be moved down (only while zoomed: the view is otherwise as tall as the page). */
    private fun maxScrollY(): Int = (computeVerticalScrollRange() - computeVerticalScrollExtent()).coerceAtLeast(0)

    /** Told (on the main thread) when the page starts or stops being wider than the view. */
    var onWide: (Boolean) -> Unit = {}
    private var wasWide = false

    /** Told (on the main thread) when the page starts or stops being taller than the view. */
    var onTall: (Boolean) -> Unit = {}
    private var wasTall = false

    /**
     * The view is as tall as its page (wrap content), and Chromium lays the page out as tall as the view divided by the
     * smallest scale. A pinch made the view taller, the page then grew to fill it with empty space, and Chromium scrolled
     * inside the view where nothing could scroll it back. So while the page is zoomed past the scale it was fitted at,
     * the view keeps the height it had when fitted and the zoomed page moves inside it ([panDown]).
     */
    private var fitScale = Float.MAX_VALUE
    private var fitHeight = 0

    /** A new document is about to be loaded: its fitted scale and height are not known yet. */
    fun newPage() {
        fitScale = Float.MAX_VALUE
        fitHeight = 0
    }

    @Suppress("DEPRECATION") // getScale: the only way to read the current zoom without JavaScript.
    private fun zoomed(): Boolean = scale > fitScale * 1.01f

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        super.onMeasure(widthMeasureSpec, heightMeasureSpec)
        @Suppress("DEPRECATION")
        val now = scale
        if (now < fitScale) fitScale = now
        if (zoomed() && fitHeight > 0) {
            setMeasuredDimension(measuredWidth, fitHeight)
        } else {
            fitHeight = measuredHeight
        }
    }
    private val slop = ViewConfiguration.get(context).scaledTouchSlop
    private var downX = 0f
    private var lastX = 0f
    private var pinched = false
    private var downY = 0f

    /** Whether the page could not move further left / right when the finger went down (see [mayHandOver]). */
    private var edgeLeftAtStart = true
    private var edgeRightAtStart = true

    /**
     * Whether what is left of a sideways drag of the finger by [fingerDx] (positive: to the right) may go on to the pager:
     * only when the page was already at that edge when the finger went down. A drag that moved the page stops at its edge.
     */
    fun mayHandOver(fingerDx: Float): Boolean = if (fingerDx > 0) edgeLeftAtStart else edgeRightAtStart

    /**
     * While the page moves sideways the WebView asks its parents to leave the gesture alone, and that is what makes a swipe
     * between conversations unreliable on a wide or zoomed mail: once the finger starts on the page, the pager never gets it.
     * A one-finger sideways drag towards a side the page cannot move to any further takes that request back, so the pager
     * (and the row swipes) get the gesture at the edge, and on a page that fits, at once.
     */
    override fun onTouchEvent(event: MotionEvent): Boolean {
        val handled = super.onTouchEvent(event)
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                downX = event.x
                downY = event.y
                lastX = event.x
                pinched = false
                edgeLeftAtStart = !canScrollHorizontally(-1)
                edgeRightAtStart = !canScrollHorizontally(1)
            }
            // A pinch is the page's: the lists and the pager around it must not take it, however the fingers move. Until the
            // next finger goes down the gesture is never handed on at an edge (the finger left is not the one that was followed).
            MotionEvent.ACTION_POINTER_DOWN -> {
                pinched = true
                parent?.requestDisallowInterceptTouchEvent(true)
            }
            MotionEvent.ACTION_MOVE -> if (event.pointerCount == 1 && !pinched) {
                // The direction is that of the last step, so a drag that turned round is judged by where it goes now.
                val step = event.x - lastX
                lastX = event.x
                if (abs(event.x - downX) > slop && abs(event.x - downX) > abs(event.y - downY) && step != 0f &&
                    !canScrollHorizontally(if (step > 0) -1 else 1) && mayHandOver(step)
                ) {
                    parent?.requestDisallowInterceptTouchEvent(false)
                }
            }
        }
        return handled
    }

    override fun onDraw(canvas: android.graphics.Canvas) {
        super.onDraw(canvas)
        val wide = maxScrollX() > 0
        if (wide != wasWide) {
            wasWide = wide
            // Not during the draw pass: the state change lays out again.
            post { onWide(wide) }
        }
        val tall = maxScrollY() > 0
        if (tall != wasTall) {
            wasTall = tall
            post { onTall(tall) }
        }
    }

    /** Moves the page by [dx] pixels to the right (negative: left), as far as it goes; returns how far it did. */
    fun pan(dx: Float): Float {
        val before = scrollX
        val target = (before + dx).roundToInt().coerceIn(0, maxScrollX())
        if (target != before) scrollTo(target, scrollY)
        return (target - before).toFloat()
    }

    /** Moves the zoomed page by [dy] pixels down (negative: up), as far as it goes; returns how far it did. */
    fun panDown(dy: Float): Float {
        val before = scrollY
        val target = (before + dy).roundToInt().coerceIn(0, maxScrollY())
        if (target != before) scrollTo(scrollX, target)
        return (target - before).toFloat()
    }
}

private class RulesClient(private val rules: PageRules) : WebViewClient() {
    override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? =
        when (val fetch = UrlPolicy.fetch(request.url.toString(), rules.allowRemote)) {
            is UrlPolicy.Fetch.Inline -> rules.inline(fetch.contentId)?.let { (type, stream) ->
                WebResourceResponse(type, null, stream)
            } ?: empty()
            UrlPolicy.Fetch.Load -> null
            is UrlPolicy.Fetch.Empty -> {
                if (fetch.remote) view.post(rules.onBlocked)
                empty()
            }
        }

    /** The view never navigates. A link the user tapped opens outside the app; a page that moves itself on is ignored. */
    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
        if (request.hasGesture()) {
            val link = UrlPolicy.link(request.url.toString())
            if (link is UrlPolicy.Link.Open) rules.openLink(link.url)
        }
        return true
    }

    /** A dead renderer must not take the app down with it; the composable makes the view again. */
    override fun onRenderProcessGone(view: WebView, detail: android.webkit.RenderProcessGoneDetail): Boolean {
        rules.onGone.run()
        return true
    }

    private fun empty() = WebResourceResponse("text/plain", "utf-8", ByteArrayInputStream(ByteArray(0)))
}
