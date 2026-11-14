package ovh.kweb.mail.ui.thread

import android.os.Build
import android.view.ViewGroup
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.isSystemInDarkTheme
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import java.io.ByteArrayInputStream
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
) {
    val dark = isSystemInDarkTheme()
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
        AndroidView(
            modifier = modifier.fillMaxWidth(),
            factory = { context ->
                WebView(context).apply {
                    layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
                    settings.javaScriptEnabled = false
                    settings.allowFileAccess = false
                    settings.allowContentAccess = false
                    settings.setGeolocationEnabled(false)
                    settings.domStorageEnabled = false
                    @Suppress("DEPRECATION")
                    settings.databaseEnabled = false
                    settings.setSupportMultipleWindows(false)
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
                val shown = html to allowRemote
                if (view.tag != shown) {
                    view.tag = shown
                    view.loadDataWithBaseURL(null, HtmlWrapper.wrap(html), "text/html", "utf-8", null)
                }
            },
            onRelease = { view ->
                view.stopLoading()
                (view.parent as? ViewGroup)?.removeView(view)
                view.destroy()
            },
        )
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
