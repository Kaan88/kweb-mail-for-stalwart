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
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalWindowInfo
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
 * The HTML of a mail, shown with everything locked down: no script of the mail runs (only the app's [FitScript]), no file or content
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
    // The second layout of a mail that came out wider than the window ([FitRules]); null for the first. Measured again
    // when the window's width or the user's text size changes (a rotation, a folding phone, split screen).
    val windowWidth = LocalWindowInfo.current.containerSize.width
    var fit by remember(html, allowRemote, flat, windowWidth, textZoom) { mutableStateOf<FitRules.Fit?>(null) }
    // The first layout of a page is not shown until it was measured: a page laid out again would flash small first.
    var ready by remember(html, allowRemote, flat, windowWidth, textZoom) { mutableStateOf(false) }
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
                .graphicsLayer { alpha = if (ready) 1f else 0f }
                .nestedScroll(edge)
                .scrollable(pan, Orientation.Horizontal, enabled = wide)
                .scrollable(panDown, Orientation.Vertical, enabled = tall),
            factory = { context ->
                PanWebView(context).apply {
                    panView[0] = this
                    onReady = { ready = true }
                    onHide = { ready = false }
                    onWide = { wide = it }
                    onTall = { tall = it }
                    layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
                    // On only for the app's own fitting script (FitScript, run by evaluateJavascript): the page's content
                    // security policy (script-src 'none') still blocks every script, handler and javascript: link of the
                    // mail, and there is no JavaScript interface.
                    settings.javaScriptEnabled = true
                    settings.javaScriptCanOpenWindowsAutomatically = false
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
                // A page laid out at the window's width is measured once it is drawn; one that came out wider is laid out
                // again at its own width (see FitRules), and its text is made larger by as much as the page is scaled down.
                // The second layout is measured too: one whose tables grew with the text gets a third (FitRules.refit).
                view.onFit = { content, window ->
                    val current = fit
                    // A mail with no table has nothing the third layout could change: no third load for it.
                    val next = when {
                        current == null -> FitRules.fit(content, window)
                        !html.contains("<table", ignoreCase = true) -> null
                        else -> FitRules.refit(current, content)
                    }
                    if (next != null) fit = next
                    next != null
                }
                val wide = fit
                view.settings.textZoom = FitRules.textZoom(textZoom, wide)
                // Until the page is shown it takes no touch (a link would open unseen) and is not read out.
                view.interactive = ready
                view.importantForAccessibility =
                    if (ready) android.view.View.IMPORTANT_FOR_ACCESSIBILITY_AUTO else android.view.View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
                val shown = listOf(html, allowRemote, textColor, flat, wide?.width, wide?.tablesBack, windowWidth, textZoom)
                if (view.tag != shown) {
                    view.tag = shown
                    view.newPage(measure = wide == null || !wide.tablesBack)
                    view.loadDataWithBaseURL(
                        null,
                        HtmlWrapper.wrap(
                            html,
                            textColor,
                            imageNote,
                            blockRemote = !allowRemote,
                            flat = flat,
                            layoutWidth = wide?.width,
                            tableZoom = FitRules.tableZoom(wide),
                        ),
                        "text/html",
                        "utf-8",
                        null,
                    )
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

/** How long a page waits to be measured before it is shown anyway. */
private const val SHOW_ANYWAY_MS = 1500L

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

    /**
     * Told (on the main thread) how wide the page came out and how wide the view is, both in CSS pixels; answers whether
     * the page will be loaded again (it then stays hidden until that one is drawn).
     */
    var onFit: (contentWidth: Float, windowWidth: Float) -> Boolean = { _, _ -> false }

    /** Told (on the main thread) that the page may be shown. */
    var onReady: () -> Unit = {}

    /** Told (on the main thread) that a page already shown is about to be loaded again, so it is hidden until that is drawn. */
    var onHide: () -> Unit = {}

    /** Whether the page takes touches: not while it is hidden. */
    var interactive = true

    /** Which load may be shown when ([PageReveal]); the callbacks are read when called, so they may be set later. */
    private val reveal = PageReveal(show = { onReady() }, hide = { onHide() })

    override fun dispatchTouchEvent(event: MotionEvent): Boolean = interactive && super.dispatchTouchEvent(event)

    /** A new document is about to be loaded: its fitted scale and height are not known yet. [measure]: report its width. */
    fun newPage(measure: Boolean = false) {
        fitScale = Float.MAX_VALUE
        fitHeight = 0
        val now = reveal.newPage(measure)
        postDelayed({ reveal.timedOut(now) }, SHOW_ANYWAY_MS)
    }

    /** The page finished loading: once it has been drawn it is measured (while it is to be) or shown. */
    fun pageFinished() {
        val now = reveal.finished()
        // The wide parts are fitted first ([FitScript]), so the page is measured and shown as it will stay. Only on the
        // app's own document (loaded from data with no base, so about:blank): the view never navigates, and anything
        // else is not fitted.
        val js = if (url == null || url == "about:blank") FitScript.JS else "0"
        evaluateJavascript(js) {
            postVisualStateCallback(0, object : WebView.VisualStateCallback() {
                override fun onComplete(requestId: Long) {
                    // The scale the page was first fitted at was that of the page before the script made it narrower
                    // (smaller): it would count as zoomed and keep the height of that first fit, cutting the mail off.
                    fitScale = Float.MAX_VALUE
                    fitHeight = 0
                    requestLayout()
                    reveal.drawn(now) { reportFit() }
                }
            })
        }
    }

    /**
     * Whether the page will be loaded again at its own width; it is then hidden until that layout is drawn. A page that
     * was shown before it could be measured (by the fallback, or a neighbour in the pager that did not report until it
     * was on screen) has been seen at its small first layout for that moment, and goes blank until the second is drawn.
     */
    private fun reportFit(): Boolean {
        @Suppress("DEPRECATION") // getScale: the only way to read the current zoom without JavaScript.
        val now = scale
        val density = resources.displayMetrics.density
        if (now <= 0f || width <= 0 || density <= 0f) return false
        // Shown already (see PageReveal.timedOut) and moved or zoomed by the user: left as it is.
        if (zoomed() || scrollX != 0) return false
        // The range is the page's width in view pixels at the current scale, whether or not it was fitted yet.
        return onFit(maxOf(computeHorizontalScrollRange(), width) / now, width / density)
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

    override fun onPageFinished(view: WebView, url: String?) {
        (view as? PanWebView)?.pageFinished()
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
