package ovh.kweb.mail.ui.admin

import androidx.annotation.StringRes
import java.util.Locale
import ovh.kweb.mail.R
import uniffi.kweb_core.DmarcCheck
import uniffi.kweb_core.Maintenance
import uniffi.kweb_core.SpamCheck
import uniffi.kweb_core.SpamTag

/** A group of the Actions screen: its label and the actions in it, in the order shown. */
data class ActionGroup(@StringRes val label: Int, val actions: List<Maintenance>)

/** The words of one simple action. */
data class ActionWords(@StringRes val name: Int, @StringRes val detail: Int, @StringRes val done: Int)

/** What was typed for the DMARC check. The two SPF domains may be left blank: they follow the other fields. */
data class DmarcForm(
    val remoteIp: String = "",
    val ehloDomain: String = "",
    val mailFrom: String = "",
    val spfEhloDomain: String = "",
    val spfMailFromDomain: String = "",
    val rcptTo: String = "",
    val message: String = "",
) {
    val canRun: Boolean get() = remoteIp.isNotBlank() && ehloDomain.isNotBlank() && mailFrom.isNotBlank()

    /** The request: a blank SPF EHLO domain is the EHLO domain, a blank SPF MAIL FROM domain is the domain of MAIL FROM. */
    fun toCheck() = DmarcCheck(
        remoteIp = remoteIp.trim(),
        ehloDomain = ehloDomain.trim(),
        mailFrom = mailFrom.trim(),
        spfEhloDomain = spfEhloDomain.trim().ifEmpty { ehloDomain.trim() },
        spfMailFromDomain = spfMailFromDomain.trim().ifEmpty { mailFrom.trim().substringAfter('@', "") },
        rcptTo = rcptTo.trim(),
        message = message,
    )
}

/** What was typed for the spam check. */
data class SpamForm(
    val message: String = "",
    val remoteIp: String = "",
    val ehloDomain: String = "",
    val mailFrom: String = "",
    val rcptTo: String = "",
    val isTls: Boolean = false,
    val authenticatedAs: String = "",
) {
    val canRun: Boolean get() =
        message.isNotBlank() && remoteIp.isNotBlank() && ehloDomain.isNotBlank() && mailFrom.isNotBlank() && rcptTo.isNotBlank()

    fun toCheck() = SpamCheck(
        message = message,
        remoteIp = remoteIp.trim(),
        ehloDomain = ehloDomain.trim(),
        mailFrom = mailFrom.trim(),
        rcptTo = rcptTo.trim(),
        isTls = isTls,
        authenticatedAs = authenticatedAs.trim(),
    )
}

/** The decisions of the Actions screens that need no Android UI: they are tested on their own. */
object ServerActionRules {
    private val groups = listOf(
        ActionGroup(
            R.string.admin_actions_reload,
            listOf(
                Maintenance.RELOAD_SETTINGS,
                Maintenance.RELOAD_CERTIFICATES,
                Maintenance.RELOAD_LOOKUP_STORES,
                Maintenance.RELOAD_BLOCKED_IPS,
                Maintenance.UPDATE_APPS,
            ),
        ),
        ActionGroup(R.string.admin_actions_caches, listOf(Maintenance.INVALIDATE_CACHES, Maintenance.INVALIDATE_NEGATIVE_CACHES)),
        ActionGroup(R.string.admin_actions_queue, listOf(Maintenance.PAUSE_QUEUE, Maintenance.RESUME_QUEUE)),
    )

    /** The groups the key may use: an action it may not start is left out, and a group with none is left out. */
    fun visible(allowed: List<Maintenance>): List<ActionGroup> =
        groups.map { g -> g.copy(actions = g.actions.filter { it in allowed }) }.filter { it.actions.isNotEmpty() }

    fun words(what: Maintenance) = when (what) {
        Maintenance.RELOAD_SETTINGS -> ActionWords(R.string.admin_reload_settings, R.string.admin_reload_settings_detail, R.string.admin_reload_settings_done)
        Maintenance.RELOAD_CERTIFICATES -> ActionWords(R.string.admin_reload_certs, R.string.admin_reload_certs_detail, R.string.admin_reload_certs_done)
        Maintenance.RELOAD_LOOKUP_STORES -> ActionWords(R.string.admin_reload_lookups, R.string.admin_reload_lookups_detail, R.string.admin_reload_lookups_done)
        Maintenance.RELOAD_BLOCKED_IPS -> ActionWords(R.string.admin_reload_blocked, R.string.admin_reload_blocked_detail, R.string.admin_reload_blocked_done)
        Maintenance.UPDATE_APPS -> ActionWords(R.string.admin_update_apps, R.string.admin_update_apps_detail, R.string.admin_update_apps_done)
        Maintenance.INVALIDATE_CACHES -> ActionWords(R.string.admin_clear_caches, R.string.admin_clear_caches_detail, R.string.admin_clear_caches_done)
        Maintenance.INVALIDATE_NEGATIVE_CACHES -> ActionWords(R.string.admin_clear_negative, R.string.admin_clear_negative_detail, R.string.admin_clear_negative_done)
        Maintenance.PAUSE_QUEUE -> ActionWords(R.string.admin_pause_queue, R.string.admin_pause_queue_detail, R.string.admin_pause_queue_done)
        Maintenance.RESUME_QUEUE -> ActionWords(R.string.admin_resume_queue, R.string.admin_resume_queue_detail, R.string.admin_resume_queue_done)
    }

    /** A spam score, one decimal. */
    fun scoreText(score: Double): String = String.format(Locale.ROOT, "%.1f", score)

    /** A tag's score with its sign ("+3.5", "-1.0"); a tag with no score is "0.0". */
    fun tagScoreText(score: Double): String = if (score > 0.04) "+" + scoreText(score) else scoreText(score)

    /** The tags that mattered: they reject, discard or change the score. The others (score zero) are only counted. */
    fun mattering(tags: List<SpamTag>): List<SpamTag> = tags.filter { it.disposition != "score" || kotlin.math.abs(it.score) >= 0.05 }

    @StringRes
    fun policyWords(policy: String): Int = when (policy) {
        "none" -> R.string.admin_dmarc_policy_none
        "quarantine" -> R.string.admin_dmarc_policy_quarantine
        "reject" -> R.string.admin_dmarc_policy_reject
        else -> R.string.admin_dmarc_policy_unset
    }

    @StringRes
    fun verdictWords(result: String): Int = when (result) {
        "spam" -> R.string.admin_spam_verdict_spam
        "ham" -> R.string.admin_spam_verdict_ham
        "reject" -> R.string.admin_spam_verdict_reject
        "discard" -> R.string.admin_spam_verdict_discard
        else -> R.string.admin_spam_verdict_unknown
    }

    /** The verdict is good news only when the message is not spam. */
    fun verdictGood(result: String): Boolean = result == "ham"

    @StringRes
    fun dispositionWords(disposition: String): Int? = when (disposition) {
        "reject" -> R.string.admin_spam_tag_rejects
        "discard" -> R.string.admin_spam_tag_discards
        else -> null
    }
}
