package ovh.kweb.mail.ui.compose

import uniffi.kweb_core.Address

/**
 * What a recipient field accepts as an address, in plain Kotlin. The validity rule is the
 * core's: exactly one "@", something before it and something after it, no blanks. Anything
 * stricter is the server's to say when the message is sent.
 */
object Recipients {
    /** What one try at turning typed text into recipients made. */
    data class Consumed(
        /** The addresses that became chips, in the order typed. */
        val accepted: List<Address>,
        /** What stays in the text field. */
        val rest: String,
        /** Something that was meant to be an address (a separator or Enter followed it) is not one, and stays in [rest]. */
        val rejected: Boolean,
    )

    fun isValid(email: String): Boolean {
        val at = email.indexOf('@')
        return at > 0 && at == email.lastIndexOf('@') && at < email.length - 1 && email.none { it.isWhitespace() }
    }

    /**
     * One address from [text]: "a@b.c", or "Name <a@b.c>" (the name may be in quotes). Null when
     * it is not a valid address. The result's address has no blanks around it.
     */
    fun parse(text: String): Address? {
        val trimmed = text.trim()
        if (trimmed.endsWith('>')) {
            val open = trimmed.lastIndexOf('<')
            if (open < 0) return null
            val email = trimmed.substring(open + 1, trimmed.length - 1).trim()
            if (!isValid(email)) return null
            val name = trimmed.substring(0, open).trim().removeSurrounding("\"").trim()
            return Address(name, email)
        }
        return if (isValid(trimmed)) Address("", trimmed) else null
    }

    /**
     * Takes the finished addresses out of what was typed. A separator (comma or semicolon outside
     * quotes and angle brackets) finishes the part before it; a blank at the end finishes a part
     * that is a whole address; [enter] finishes everything. A part that is not an address stops
     * the walk: it and what follows stay as text, without the separator after it, and [Consumed.rejected] says so.
     */
    fun consume(text: String, enter: Boolean = false): Consumed {
        val accepted = mutableListOf<Address>()
        val parts = split(text)
        for ((index, part) in parts.withIndex()) {
            val last = index == parts.lastIndex
            val trimmed = part.text.trim()
            if (!last) {
                if (trimmed.isEmpty()) continue
                val address = parse(trimmed) ?: return Consumed(accepted, text.substring(part.start).trim().trimEnd(',', ';').trimEnd(), true)
                accepted += address
                continue
            }
            if (trimmed.isEmpty()) return Consumed(accepted, "", false)
            val finished = enter || part.text.last().isWhitespace()
            val address = if (finished) parse(trimmed) else null
            return when {
                address != null -> Consumed(accepted + address, "", false)
                else -> Consumed(accepted, part.text.trimStart(), enter)
            }
        }
        return Consumed(accepted, "", false)
    }

    /** [added] after [existing], without an address that is there already (case does not matter). */
    fun merge(existing: List<Address>, added: List<Address>): List<Address> {
        val seen = existing.map { it.email.lowercase() }.toMutableSet()
        return existing + added.filter { seen.add(it.email.lowercase()) }
    }

    /** The text left in a field is not an address (and not nothing): it must be fixed or cleared before sending. */
    fun blocksSend(rest: String): Boolean = rest.isNotBlank() && parse(rest) == null

    /** Whether the field's leftover text is drawn as wrong: a try to finish it was made and it is still not an address. */
    fun isMarked(rest: String, rejectedBefore: Boolean): Boolean = rejectedBefore && blocksSend(rest)

    private class Part(val text: String, val start: Int)

    /** The parts of [text] between separators; the last has none after it. */
    private fun split(text: String): List<Part> {
        val parts = mutableListOf<Part>()
        var start = 0
        var quoted = false
        var angle = false
        for ((i, c) in text.withIndex()) {
            when {
                c == '"' -> quoted = !quoted
                quoted -> Unit
                c == '<' -> angle = true
                c == '>' -> angle = false
                (c == ',' || c == ';') && !angle -> {
                    parts += Part(text.substring(start, i), start)
                    start = i + 1
                }
            }
        }
        parts += Part(text.substring(start), start)
        return parts
    }
}
