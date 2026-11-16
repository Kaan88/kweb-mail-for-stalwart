package ovh.kweb.mail.ui.filters

import java.util.UUID
import uniffi.kweb_core.Core
import uniffi.kweb_core.Exception as CoreException

/**
 * "Block sender": a rule "Blocked: address" (From address is it, move to the Trash, stop) at the top of the filter
 * rules, written through the same calls as the editor. The pure parts are here so that they can be tested.
 */
internal object BlockRules {
    fun name(address: String) = "Blocked: $address"

    fun rule(address: String, trash: FolderRow): Map<String, Any?> = linkedMapOf(
        "id" to UUID.randomUUID().toString(),
        "name" to name(address),
        "enabled" to true,
        "matchType" to "all",
        "conditions" to listOf(linkedMapOf("field" to "from", "comparator" to "address_is", "value" to address)),
        "actions" to listOf(linkedMapOf("type" to "move", "value" to trash.path, "mailboxId" to trash.id)),
        "stopProcessing" to true,
        "includeSpam" to true,
    )

    /** Whether an enabled rule already stops mail from exactly [address]. */
    fun alreadyBlocked(rules: List<Any?>, address: String): Boolean = rules.any { r ->
        val o = Json.obj(r) ?: return@any false
        if (o["enabled"] != true || o["matchType"] != "all") return@any false
        val conditions = Json.arr(o["conditions"]).orEmpty().mapNotNull { Json.obj(it) }
        val actions = Json.arr(o["actions"]).orEmpty().mapNotNull { Json.obj(it) }
        conditions.size == 1 && conditions[0]["field"] == "from" && conditions[0]["comparator"] == "address_is" &&
            (conditions[0]["value"] as? String).equals(address, ignoreCase = true) &&
            actions.any { it["type"] == "discard" || it["type"] == "move" }
    }

    /** The rules JSON with the block rule first, or null when the sender is blocked already. */
    fun withBlock(rulesJson: String, address: String, trash: FolderRow): String? {
        val rules = Json.arr(Json.parse(rulesJson)).orEmpty()
        if (alreadyBlocked(rules, address)) return null
        return Json.write(listOf(rule(address, trash)) + rules)
    }

    /** Whether [address] is one of the user's own (blocking it would send their own mail to the Trash). */
    fun isOwn(address: String, own: Collection<String>) = own.any { it.trim().equals(address.trim(), ignoreCase = true) }
}

internal object BlockSender {
    private fun refuse(text: String): Nothing = throw CoreException.Invalid(text)

    /**
     * Puts the block rule first and switches the filters on. Blocking and the words for a refusal are decided here; a
     * refusal is a [CoreException.Invalid] with words for the user. Returns whether a rule was added (false: already blocked).
     * Blocking, off the main thread.
     */
    fun block(core: Core, address: String): Boolean {
        val sender = address.trim()
        if (sender.isEmpty() || sender.any { it.isISOControl() }) refuse("That message has no sender address to block.")
        val own = HashSet<String>()
        core.account()?.username?.let(own::add)
        try {
            core.identities().forEach { own += it.email }
        } catch (e: CoreException.Invalid) {
            // The sign-in name stands in.
        }
        if (BlockRules.isOwn(sender, own)) refuse("That is one of your own addresses. Blocking it would send your own mail to the Trash.")
        val state = core.filtersState()
        if (!state.supported) refuse("This server has no mail filters, so a sender cannot be blocked here.")
        if (state.opaque || state.foreign) refuse("The filter script was not made with the filter editor, so a rule cannot be added. Open the mail filters to deal with it first.")
        if (state.migrationPending) refuse("Your old filters have not been moved yet. Open the mail filters and move them first.")
        if (!state.active && state.vacationConflict) refuse("The away message is on and cannot run beside the filters. Switch it off or open the mail filters.")
        val trash = RuleModel.folderTree(core.mailboxes()).let { tree ->
            core.mailboxes().firstOrNull { it.role == "trash" }?.let { t -> tree.firstOrNull { it.id == t.id } }
        } ?: refuse("The account has no Trash folder.")
        val json = BlockRules.withBlock(state.rulesJson, sender, trash)
        val saved = if (json != null) core.filtersSave(json) else state
        if (!saved.active) core.filtersSetEnabled(true)
        return json != null
    }
}
