package ovh.kweb.mail.background

import uniffi.kweb_core.MailNotification
import uniffi.kweb_core.Plan

/*
 * The decisions [Notifier] makes about a plan, as plain functions so they can be tested on the JVM.
 */

/**
 * The part of [Plan] the user's switches allow: with mail off nothing is posted, and with
 * "remove when read elsewhere" off the cancellations are not applied.
 */
internal fun Plan.honouring(mailEnabled: Boolean, removeWhenReadElsewhere: Boolean): Plan = Plan(
    post = if (mailEnabled) post else emptyList(),
    cancel = if (removeWhenReadElsewhere) cancel else emptyList(),
)

/**
 * Whether a posting must be silent. Besides what the core says, a notification that is already
 * on screen is only refreshed: this makes applying the same plan twice (the core hands a check's
 * plan both to the caller and to the listener) harmless even if a channel's sound is on.
 */
internal fun isSilent(notification: MailNotification, onScreen: Set<Int>): Boolean =
    notification.silent || notification.androidId in onScreen

/** The Android ids of mail notifications that will be on screen once [plan] has been applied. */
internal fun idsAfter(onScreen: Set<Int>, plan: Plan): Set<Int> =
    (onScreen + plan.post.map { it.androidId }) - plan.cancel.map { it.androidId }.toSet()

/** The core counts seconds; Android wants milliseconds. An unknown time (0) means "now". */
internal fun whenMillis(receivedAtSeconds: Long, nowMillis: Long): Long =
    if (receivedAtSeconds > 0) receivedAtSeconds * 1000 else nowMillis
