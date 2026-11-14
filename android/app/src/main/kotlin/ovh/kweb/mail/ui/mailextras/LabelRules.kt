package ovh.kweb.mail.ui.mailextras

import uniffi.kweb_core.Label
import uniffi.kweb_core.MessageLabels

/** Whether the messages being labelled carry a label: all of them, some, or none. */
enum class Carry { ALL, SOME, NONE }

object LabelRules {
    const val MAX_NAME = 60

    /** Chips on a row of the list before "+n". */
    const val ROW_CHIPS = 3

    /** Why [name] cannot be a label, as the app can tell before asking the core; null when it can. The core has the last word. */
    enum class NameProblem { BLANK, TOO_LONG, MULTILINE }

    fun nameProblem(name: String): NameProblem? = when {
        name.trim().isEmpty() -> NameProblem.BLANK
        name.any { it == '\n' || it == '\r' } -> NameProblem.MULTILINE
        name.trim().codePointCount(0, name.trim().length) > MAX_NAME -> NameProblem.TOO_LONG
        else -> null
    }

    /** What [labelId] is on [messageIds]: [Carry.NONE] for an empty list. */
    fun carry(messageIds: List<String>, byMessage: Map<String, List<String>>, labelId: String): Carry {
        if (messageIds.isEmpty()) return Carry.NONE
        val with = messageIds.count { labelId in byMessage[it].orEmpty() }
        return when (with) {
            0 -> Carry.NONE
            messageIds.size -> Carry.ALL
            else -> Carry.SOME
        }
    }

    /** True when the labels of every one of [messageIds] have been read (a message with no entry has not been read yet). */
    fun known(messageIds: List<String>, byMessage: Map<String, List<String>>): Boolean =
        messageIds.isNotEmpty() && messageIds.all { byMessage.containsKey(it) }

    /** Of [shownIds], those that still carry [labelId] according to [byMessage]; a message not read about stays. */
    fun stillCarrying(shownIds: List<String>, byMessage: Map<String, List<String>>, labelId: String): List<String> =
        shownIds.filter { id -> byMessage[id]?.contains(labelId) ?: true }

    /** A tap on the check row: take the label off when every message has it, else put it on all of them. */
    fun turnOn(carry: Carry): Boolean = carry != Carry.ALL

    /** The names of the labels of one message, in the order of the user's list; a label that is not in the list is not drawn. */
    fun namesOf(labelIds: List<String>?, labels: List<Label>): List<String> {
        if (labelIds.isNullOrEmpty()) return emptyList()
        return labels.filter { it.id in labelIds }.map { it.name }
    }

    /** The names for a row of several messages: every label that any of them carries, once. */
    fun namesOfAny(messageIds: List<String>, byMessage: Map<String, List<String>>, labels: List<Label>): List<String> {
        val ids = HashSet<String>()
        messageIds.forEach { byMessage[it]?.let(ids::addAll) }
        return if (ids.isEmpty()) emptyList() else labels.filter { it.id in ids }.map { it.name }
    }

    /** Splits the chips of a row into those drawn and how many are left out. */
    fun shown(names: List<String>): Pair<List<String>, Int> = names.take(ROW_CHIPS) to (names.size - ROW_CHIPS).coerceAtLeast(0)

    /** The answer of the core as a map; a message it says nothing about has no label. */
    fun index(requested: List<String>, answer: List<MessageLabels>): Map<String, List<String>> {
        val given = answer.associate { it.messageId to it.labelIds }
        return requested.associateWith { given[it].orEmpty() }
    }

    /** [ids] with [labelId] put on or taken off, for the picture shown before the core has answered. */
    fun applied(byMessage: Map<String, List<String>>, messageIds: List<String>, labelId: String, on: Boolean): Map<String, List<String>> {
        val out = HashMap(byMessage)
        for (id in messageIds) {
            val current = out[id].orEmpty()
            out[id] = if (on) (if (labelId in current) current else current + labelId) else current - labelId
        }
        return out
    }
}
