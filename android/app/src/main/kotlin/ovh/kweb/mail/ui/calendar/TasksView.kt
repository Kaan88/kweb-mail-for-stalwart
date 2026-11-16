package ovh.kweb.mail.ui.calendar

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import java.time.LocalDate
import java.time.ZoneId
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.Occurrence

/**
 * The Tasks view of the Calendar tab: the open tasks as groups (Overdue, Today, Tomorrow, Later), each a row with a round box
 * and the due word at the end. [top] adds items above the list (the "Worth a look" box when it is open).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TasksBody(
    state: TasksState,
    today: LocalDate,
    zone: ZoneId,
    onOpen: (Occurrence) -> Unit,
    onToggle: (Occurrence, Boolean) -> Unit,
    onRefresh: () -> Unit,
    modifier: Modifier = Modifier,
    top: (LazyListScope.() -> Unit)? = null,
    listState: androidx.compose.foundation.lazy.LazyListState = androidx.compose.foundation.lazy.rememberLazyListState(),
) {
    val locale = rememberLocale()
    val sections = remember(state.entries, today, zone) { TaskRules.sections(state.entries, today, zone) }
    val todayWord = stringResource(R.string.cal_tasks_today)
    PullToRefreshBox(isRefreshing = state.refreshing, onRefresh = onRefresh, modifier = modifier) {
        LazyColumn(Modifier.fillMaxSize(), state = listState, contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 96.dp)) {
            top?.invoke(this)
            when {
                !state.loaded && !state.failed -> item(key = "loading") {
                    Column(Modifier.fillMaxWidth().padding(32.dp), Arrangement.Center, Alignment.CenterHorizontally) { CircularProgressIndicator() }
                }
                !state.loaded -> item(key = "failed") {
                    Column(Modifier.fillMaxWidth().padding(32.dp), Arrangement.Center, Alignment.CenterHorizontally) {
                        Text(stringResource(R.string.cal_tasks_load_failed), color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
                        TextButton(onClick = onRefresh) { Text(stringResource(R.string.retry)) }
                    }
                }
                sections.isEmpty() -> item(key = "empty") {
                    Text(
                        stringResource(R.string.cal_tasks_empty),
                        modifier = Modifier.fillMaxWidth().padding(32.dp),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                    )
                }
                else -> for (section in sections) {
                    item(key = "head/${section.bucket}") { SectionHeading(section) }
                    items(section.items.size, key = { "task/${TaskRules.key(section.items[it])}" }) { index ->
                        val entry = section.items[index]
                        TaskRow(
                            entry = entry,
                            due = TaskRules.dueText(entry, today, zone, locale, todayWord),
                            late = TaskRules.isLate(section.bucket),
                            shape = Groups.shape(index, section.items.size),
                            onOpen = { onOpen(entry) },
                            onToggle = { onToggle(entry, it) },
                            modifier = Modifier.padding(bottom = 2.dp),
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun SectionHeading(section: TaskRules.Section) {
    val word = stringResource(
        when (section.bucket) {
            TaskRules.Bucket.OVERDUE -> R.string.cal_tasks_overdue
            TaskRules.Bucket.TODAY -> R.string.cal_tasks_today
            TaskRules.Bucket.TOMORROW -> R.string.cal_tasks_tomorrow
            TaskRules.Bucket.LATER -> R.string.cal_tasks_later
        },
    )
    val colors = MaterialTheme.colorScheme
    Row(Modifier.padding(start = 12.dp, top = 12.dp, bottom = 4.dp).semantics { heading() }) {
        Text(word, style = MaterialTheme.typography.labelLarge, color = colors.onSurfaceVariant, fontWeight = FontWeight.SemiBold)
        if (section.bucket == TaskRules.Bucket.OVERDUE || section.bucket == TaskRules.Bucket.TODAY) {
            Text(" · ${section.items.size}", style = MaterialTheme.typography.labelLarge, color = colors.onSurfaceVariant)
        }
    }
}

@Composable
private fun TaskRow(
    entry: Occurrence,
    due: String,
    late: Boolean,
    shape: Shape,
    onOpen: () -> Unit,
    onToggle: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = MaterialTheme.colorScheme
    Row(
        modifier.fillMaxWidth().clip(shape).background(colors.surfaceContainerLow).clickable(onClick = onOpen).padding(start = 6.dp, end = 16.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        TaskBox(done = entry.isDone, color = colors.onSurfaceVariant, checkColor = colors.surfaceContainerLow, onToggle = { onToggle(!entry.isDone) })
        Text(
            entry.title.ifBlank { stringResource(R.string.cal_untitled) },
            modifier = Modifier.weight(1f).padding(end = 8.dp),
            style = MaterialTheme.typography.bodyMedium,
            textDecoration = if (entry.isDone) TextDecoration.LineThrough else null,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
        // The due date is data: in the error colour once it has passed.
        Text(due, style = MaterialTheme.typography.labelMedium, color = if (late) colors.error else colors.onSurfaceVariant, maxLines = 1)
    }
}
