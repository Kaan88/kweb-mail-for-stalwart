package ovh.kweb.mail.ui.thread

import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.pdf.PdfRenderer
import android.os.ParcelFileDescriptor
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculatePan
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import java.io.File
import java.util.concurrent.Executors

/** How far a PDF can be zoomed in, and the widest page bitmap (pixels) made for it. */
internal object PdfViewRules {
    const val MAX_ZOOM = 4f
    const val MAX_BITMAP_WIDTH = 2400

    /** The zoom after a pinch by [factor], kept between 1 (pages fit the width) and [MAX_ZOOM]. */
    fun zoom(current: Float, factor: Float): Float = (current * factor).coerceIn(1f, MAX_ZOOM)

    /** The sideways offset kept inside the zoomed page: the page may move by at most the part that sticks out. */
    fun offset(offset: Float, zoom: Float, width: Float): Float {
        val room = (width * zoom - width) / 2f
        return offset.coerceIn(-room, room)
    }

    /** The most pixels one page bitmap may have (48 MB): Android refuses to draw a bitmap of more than 100 MB. */
    const val MAX_PIXELS = 12_000_000L

    /** A page shape until the page itself was read: A4 upright. */
    const val DEFAULT_RATIO = 1f / 1.414f

    /** The width of a page bitmap: sharp at twice the screen width, never wider than [MAX_BITMAP_WIDTH]. */
    fun bitmapWidth(viewWidth: Int): Int = (viewWidth * 2).coerceIn(1, MAX_BITMAP_WIDTH)

    /**
     * Width and height of the bitmap for a page of [pageWidth] by [pageHeight] points drawn [width] pixels wide: the page's
     * shape kept, and made smaller as a whole when it would have more than [MAX_PIXELS] (a very tall page, a long receipt).
     */
    fun bitmapSize(width: Int, pageWidth: Int, pageHeight: Int): Pair<Int, Int> {
        val w = width.coerceAtLeast(1).toLong()
        val h = (w * pageHeight.coerceAtLeast(1) / pageWidth.coerceAtLeast(1)).coerceAtLeast(1)
        if (w * h <= MAX_PIXELS) return w.toInt() to h.toInt()
        val scale = kotlin.math.sqrt(MAX_PIXELS.toDouble() / (w * h))
        return (w * scale).toInt().coerceAtLeast(1) to (h * scale).toInt().coerceAtLeast(1)
    }

    /** A page's width over height, or [DEFAULT_RATIO] for a page with no usable size. */
    fun ratio(pageWidth: Int, pageHeight: Int): Float =
        if (pageWidth <= 0 || pageHeight <= 0) DEFAULT_RATIO else pageWidth.toFloat() / pageHeight
}

/**
 * A PDF shown in the app, one page under the other, pinch to zoom. The file is the attachment already in the cache;
 * nothing leaves the app. [onOpenWith] hands it to another app instead.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PdfViewerDialog(file: File, title: String, onOpenWith: () -> Unit, onDismiss: () -> Unit) {
    // PdfRenderer allows one open page at a time and is not thread-safe: one thread and a lock.
    val renderer = remember(file) { runCatching { PdfDocument(file) }.getOrNull() }
    DisposableEffect(renderer) { onDispose { renderer?.close() } }
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Scaffold(
            topBar = {
                TopAppBar(
                    title = { Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    navigationIcon = {
                        IconButton(onClick = onDismiss) { Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.pdf_close)) }
                    },
                    actions = { TextButton(onClick = onOpenWith) { Text(stringResource(R.string.pdf_open_with)) } },
                )
            },
        ) { padding ->
            if (renderer == null) {
                Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
                    Text(stringResource(R.string.pdf_cannot_show), style = MaterialTheme.typography.bodyLarge)
                }
            } else {
                Pages(renderer, Modifier.fillMaxSize().padding(padding))
            }
        }
    }
}

@Composable
private fun Pages(doc: PdfDocument, modifier: Modifier) {
    var zoom by remember { mutableFloatStateOf(1f) }
    var offsetX by remember { mutableFloatStateOf(0f) }
    var offsetY by remember { mutableFloatStateOf(0f) }
    val list = rememberLazyListState()
    BoxWithConstraints(modifier.background(MaterialTheme.colorScheme.surfaceContainerHighest)) {
        val width = constraints.maxWidth.toFloat()
        val height = constraints.maxHeight.toFloat()
        val bitmapWidth = PdfViewRules.bitmapWidth(constraints.maxWidth)
        // Zoomed in, the list is scaled about its centre, so its top and bottom bands lie outside the view. What the list
        // cannot scroll any further (at its top or its end, or a page that fits) moves the zoomed list up and down instead.
        val edge = remember(list, height) {
            object : NestedScrollConnection {
                override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset {
                    if (zoom <= 1f || available.y == 0f) return Offset.Zero
                    val before = offsetY
                    // The list scrolls in its own (unscaled) units; the offset is in screen pixels.
                    offsetY = PdfViewRules.offset(offsetY + available.y * zoom, zoom, height)
                    return Offset(0f, (offsetY - before) / zoom)
                }
            }
        }
        LazyColumn(
            state = list,
            contentPadding = PaddingValues(vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier
                .fillMaxSize()
                // Two fingers zoom (and move); one finger is left to the list (up and down) and to the drag below.
                .pointerInput(Unit) {
                    awaitEachGesture {
                        awaitFirstDown(requireUnconsumed = false)
                        do {
                            val event = awaitPointerEvent()
                            if (event.changes.count { it.pressed } >= 2) {
                                zoom = PdfViewRules.zoom(zoom, event.calculateZoom())
                                val pan = event.calculatePan()
                                offsetX = PdfViewRules.offset(offsetX + pan.x, zoom, width)
                                val scroll = -pan.y / zoom
                                val left = scroll - list.dispatchRawDelta(scroll)
                                offsetY = PdfViewRules.offset(offsetY - left * zoom, zoom, height)
                                event.changes.forEach { it.consume() }
                            }
                        } while (event.changes.any { it.pressed })
                    }
                }
                .draggable(
                    rememberDraggableState { offsetX = PdfViewRules.offset(offsetX + it, zoom, width) },
                    Orientation.Horizontal,
                    enabled = zoom > 1f,
                )
                .graphicsLayer {
                    scaleX = zoom
                    scaleY = zoom
                    translationX = offsetX
                    translationY = offsetY
                }
                .nestedScroll(edge),
        ) {
            items(doc.pageCount) { index -> Page(doc, index, bitmapWidth) }
        }
    }
}

@Composable
private fun Page(doc: PdfDocument, index: Int, bitmapWidth: Int) {
    // Read on the document's own thread, never in composition: a page being rendered holds the renderer.
    val ratio by produceState(PdfViewRules.DEFAULT_RATIO, index) { value = doc.ratio(index) }
    // Null while it is being made; a page that failed (too large, broken) says so instead of turning forever.
    val bitmap by produceState<Result<Bitmap?>?>(null, index, bitmapWidth) {
        value = runCatching { doc.render(index, bitmapWidth) }
    }
    val description = stringResource(R.string.pdf_page, index + 1, doc.pageCount)
    Box(
        Modifier.fillMaxWidth().padding(horizontal = 8.dp).aspectRatio(ratio).background(MaterialTheme.colorScheme.surface),
        contentAlignment = Alignment.Center,
    ) {
        val result = bitmap
        val image = result?.getOrNull()
        if (result == null) {
            CircularProgressIndicator()
        } else if (image == null) {
            Text(stringResource(R.string.pdf_cannot_show), style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(16.dp))
        } else {
            Image(image.asImageBitmap(), contentDescription = description, contentScale = ContentScale.FillWidth, modifier = Modifier.fillMaxWidth())
        }
    }
}

/** One open PDF. Pages are rendered one at a time on a thread of its own. */
private class PdfDocument(file: File) {
    private val descriptor = ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)
    private val renderer = try {
        PdfRenderer(descriptor)
    } catch (e: Exception) {
        descriptor.close()
        throw e
    }
    private val executor = Executors.newSingleThreadExecutor()
    private val dispatcher = executor.asCoroutineDispatcher()
    private val lock = Mutex()
    private val ratios = HashMap<Int, Float>() // touched only on the document's thread, under the lock
    @Volatile private var closed = false

    val pageCount: Int = renderer.pageCount

    /** Width over height of page [index] (read once, then remembered); [PdfViewRules.DEFAULT_RATIO] when it cannot be read. */
    suspend fun ratio(index: Int): Float = withContext(dispatcher) {
        lock.withLock {
            ratios.getOrPut(index) {
                if (closed) return@getOrPut PdfViewRules.DEFAULT_RATIO
                runCatching { renderer.openPage(index).use { page -> PdfViewRules.ratio(page.width, page.height) } }
                    .getOrDefault(PdfViewRules.DEFAULT_RATIO)
            }
        }
    }

    suspend fun render(index: Int, width: Int): Bitmap? = withContext(dispatcher) {
        lock.withLock {
            if (closed) return@withLock null
            renderer.openPage(index).use { page ->
                val (w, h) = PdfViewRules.bitmapSize(width, page.width, page.height)
                val bitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
                bitmap.eraseColor(Color.WHITE)
                page.render(bitmap, null, null, PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY)
                bitmap
            }
        }
    }

    /** Closes the document on its own thread, after the page being rendered: the main thread never waits for it. */
    fun close() {
        closed = true
        runCatching {
            executor.execute {
                runCatching { renderer.close() }
                runCatching { descriptor.close() }
            }
        }
        executor.shutdown()
    }
}
