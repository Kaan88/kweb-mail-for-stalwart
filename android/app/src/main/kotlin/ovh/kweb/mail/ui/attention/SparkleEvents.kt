package ovh.kweb.mail.ui.attention

import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.platform.LocalResources
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import ovh.kweb.mail.R

/** How long Undo is offered after a card was added. */
const val UNDO_MILLIS = 10_000L

/**
 * What the runs say while a screen shows them: the line with Undo after an Add (offered for 10 seconds), a finding that
 * is done (it stays away), a reply draft to open. One screen at a time collects them: pass [active] false for a screen
 * that is only next to the one on show.
 */
@Composable
fun SparkleEvents(
    sparkle: SparkleRunViewModel,
    attention: AttentionViewModel,
    host: SnackbarHostState,
    onOpenDraft: (String) -> Unit,
    active: Boolean = true,
) {
    val resources = LocalResources.current
    LaunchedEffect(sparkle, active) {
        if (!active) return@LaunchedEffect
        sparkle.events.collect { event ->
            when (event) {
                is SparkleEvent.Notice -> {
                    host.currentSnackbarData?.dismiss()
                    // Collecting goes on while the line is up: a later notice replaces it.
                    launch {
                        val undo = event.undo
                        if (undo == null) {
                            host.showSnackbar(event.text, duration = SnackbarDuration.Short)
                        } else {
                            val result = withTimeoutOrNull(UNDO_MILLIS) {
                                host.showSnackbar(event.text, actionLabel = resources.getString(R.string.att_undo), duration = SnackbarDuration.Indefinite)
                            }
                            if (result == SnackbarResult.ActionPerformed) undo()
                        }
                    }
                }
                is SparkleEvent.Completed -> attention.dismiss(event.finding)
                is SparkleEvent.OpenDraft -> onOpenDraft(event.draftId)
            }
        }
    }
}
