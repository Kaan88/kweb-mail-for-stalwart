package ovh.kweb.mail.ui.sender

import androidx.annotation.StringRes
import ovh.kweb.mail.R
import uniffi.kweb_core.MethodResult
import uniffi.kweb_core.SenderCheck
import uniffi.kweb_core.SenderReason
import uniffi.kweb_core.SenderStatus

/** What the sender mark of a message shows, worked out from the core's answer. No Android here: tested on the JVM. */
object SenderRules {
    enum class Tone { GOOD, NEUTRAL, BAD }

    /** A message the server did not check (mail from a local account, or one it wrote no results for) shows no mark. */
    fun shows(check: SenderCheck): Boolean = check.status != SenderStatus.NOT_CHECKED

    fun tone(check: SenderCheck): Tone = when (check.status) {
        SenderStatus.VERIFIED -> Tone.GOOD
        SenderStatus.FAILED -> Tone.BAD
        else -> Tone.NEUTRAL
    }

    /** The warning line under the sender for a failed check; the others have none. */
    fun warns(check: SenderCheck): Boolean = check.status == SenderStatus.FAILED

    @StringRes
    fun chipText(check: SenderCheck): Int = when (check.status) {
        SenderStatus.VERIFIED -> R.string.sender_chip_verified
        SenderStatus.FAILED -> R.string.sender_chip_failed
        else -> R.string.sender_chip_unverified
    }

    /** The one plain sentence of the sheet, about this status. */
    @StringRes
    fun why(check: SenderCheck): Int = when (check.reason) {
        SenderReason.DMARC_PASS -> R.string.sender_why_dmarc_pass
        SenderReason.DKIM_PASS -> R.string.sender_why_dkim_pass
        SenderReason.SPF_PASS -> R.string.sender_why_spf_pass
        SenderReason.DMARC_FAIL -> R.string.sender_why_dmarc_fail
        SenderReason.DKIM_FAIL -> R.string.sender_why_dkim_fail
        SenderReason.SPF_FAIL -> R.string.sender_why_spf_fail
        SenderReason.NOT_ALIGNED -> R.string.sender_why_not_aligned
        SenderReason.NO_RESULTS -> R.string.sender_why_no_results
        SenderReason.NOT_FROM_THIS_SERVER -> R.string.sender_why_not_checked
        SenderReason.NO_SENDER -> R.string.sender_why_no_sender
    }

    /** One method's result in a word, for the sheet's three lines. */
    @StringRes
    fun method(result: MethodResult): Int = when (result) {
        MethodResult.PASS -> R.string.sender_method_pass
        MethodResult.FAIL -> R.string.sender_method_fail
        MethodResult.OTHER -> R.string.sender_method_other
        MethodResult.NOT_RUN -> R.string.sender_method_not_run
    }
}
