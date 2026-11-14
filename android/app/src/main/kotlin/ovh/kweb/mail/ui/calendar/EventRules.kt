package ovh.kweb.mail.ui.calendar

import uniffi.kweb_core.Participant

/** Small decisions of the event screen. */
object EventRules {
    /** The user was invited by someone else and can answer. */
    fun isInvitation(participants: List<Participant>, myStatus: String?): Boolean =
        myStatus != null && participants.any { it.isMe && !it.isOrganizer }

    /** What the delete button asks. */
    enum class DeleteAsk {
        /** One entry that does not repeat. */
        PLAIN,

        /** One occurrence of a repeating event was opened: only this one, or all of them. */
        ONE_OR_ALL,

        /** A repeating event opened without a particular occurrence: it goes with all its repeats. */
        WHOLE_SERIES,
    }

    fun deleteAsk(repeats: Boolean, recurrenceId: String?): DeleteAsk = when {
        !repeats -> DeleteAsk.PLAIN
        recurrenceId != null -> DeleteAsk.ONE_OR_ALL
        else -> DeleteAsk.WHOLE_SERIES
    }

    /** The organizer first, then the user, then the others by name. */
    fun sorted(participants: List<Participant>): List<Participant> =
        participants.sortedWith(
            compareBy<Participant>({ !it.isOrganizer }, { !it.isMe }, { label(it).lowercase() }),
        )

    /** The name, or the address when there is no name. */
    fun label(participant: Participant): String = participant.name.trim().ifEmpty { participant.email.trim() }
}
