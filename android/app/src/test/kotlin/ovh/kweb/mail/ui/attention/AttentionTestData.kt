package ovh.kweb.mail.ui.attention

import uniffi.kweb_core.AttentionFamily
import uniffi.kweb_core.AttentionRule
import uniffi.kweb_core.Finding
import uniffi.kweb_core.HomeTile
import uniffi.kweb_core.PhoneAction
import uniffi.kweb_core.ServiceAction
import uniffi.kweb_core.TimeSlot

/** A finding with the fields a test cares about; the rest is empty. */
internal fun finding(
    rule: AttentionRule,
    family: AttentionFamily,
    tile: HomeTile,
    id: String = "x-${rule.name}",
    names: List<String> = emptyList(),
    times: List<Long> = emptyList(),
    count: Int = 0,
    messageIds: List<String> = emptyList(),
    threadId: String? = null,
    phone: List<PhoneAction> = emptyList(),
    service: ServiceAction? = null,
    slots: List<TimeSlot> = emptyList(),
) = Finding(
    id = id, rule = rule, family = family, tile = tile, names = names, times = times, count = count.toUInt(), at = times.firstOrNull() ?: 0L,
    eventIds = emptyList(), taskIds = emptyList(), messageIds = messageIds, threadId = threadId, phone = phone, service = service, slots = slots,
)

/** Words as ids and arguments, so a test sees which string and which values a sentence is made of. */
internal object FakeTexts : Texts {
    override fun string(res: Int, vararg args: Any): String = "s$res:" + args.joinToString("|")
    override fun plural(res: Int, quantity: Int, vararg args: Any): String = "p$res:$quantity:" + args.joinToString("|")
}
