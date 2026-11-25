package ovh.kweb.mail.ui.assistant

import java.util.Locale
import uniffi.kweb_core.ReplyPreset

/**
 * The ready-made wishes for a reply draft: chips the user taps before "Write draft". The words sent for each are the
 * core's own (`prompts::preset_line`); this side only says which were tapped. At most one of each group is on, as
 * the core requires: tapping another of the same group replaces it.
 */
object ReplyPresetRules {
    enum class Group { CONTENT, LENGTH, TONE, LANGUAGE }

    /** In the order the chips are shown and the presets are sent. */
    enum class Choice(val group: Group) {
        ACCEPT(Group.CONTENT),
        DECLINE(Group.CONTENT),
        ASK_DETAILS(Group.CONTENT),
        MEETING(Group.CONTENT),
        THANKS(Group.CONTENT),
        SHORT(Group.LENGTH),
        FORMAL(Group.TONE),
        FRIENDLY(Group.TONE),
        MY_LANGUAGE(Group.LANGUAGE),
    }

    /** [chosen] with [choice] switched: on replaces any other of its group, off removes it. */
    fun toggle(chosen: Set<Choice>, choice: Choice): Set<Choice> =
        if (choice in chosen) chosen - choice else chosen.filterTo(mutableSetOf()) { it.group != choice.group } + choice

    /**
     * What goes into the task, in a fixed order. [locale] is the app's language: "In my language" names it in English
     * (the language of the app's own instructions to the model), so that "Türkçe" is sent as "Turkish".
     */
    fun presets(chosen: Set<Choice>, locale: Locale): List<ReplyPreset> = Choice.entries.filter { it in chosen }.mapNotNull { choice ->
        when (choice) {
            Choice.ACCEPT -> ReplyPreset.Accept
            Choice.DECLINE -> ReplyPreset.Decline
            Choice.ASK_DETAILS -> ReplyPreset.AskDetails
            Choice.MEETING -> ReplyPreset.ProposeMeeting
            Choice.THANKS -> ReplyPreset.Thanks
            Choice.SHORT -> ReplyPreset.Short
            Choice.FORMAL -> ReplyPreset.Formal
            Choice.FRIENDLY -> ReplyPreset.Friendly
            Choice.MY_LANGUAGE -> languageName(locale)?.let { ReplyPreset.InLanguage(it) }
        }
    }

    /** The choices a task already carries (for "Try again" and a rotation). [InLanguage] always maps back to the chip. */
    fun choices(presets: List<ReplyPreset>): Set<Choice> = presets.mapTo(mutableSetOf()) { preset ->
        when (preset) {
            ReplyPreset.Accept -> Choice.ACCEPT
            ReplyPreset.Decline -> Choice.DECLINE
            ReplyPreset.AskDetails -> Choice.ASK_DETAILS
            ReplyPreset.ProposeMeeting -> Choice.MEETING
            ReplyPreset.Thanks -> Choice.THANKS
            ReplyPreset.Short -> Choice.SHORT
            ReplyPreset.Formal -> Choice.FORMAL
            ReplyPreset.Friendly -> Choice.FRIENDLY
            is ReplyPreset.InLanguage -> Choice.MY_LANGUAGE
        }
    }

    /** The app language's English name, or null when the locale names none. */
    fun languageName(locale: Locale): String? = locale.getDisplayLanguage(Locale.ENGLISH).trim().ifEmpty { null }

    /** The chip's own label for "In my language": the language named in itself, first letter capital ("Türkçe"). */
    fun ownLanguageName(locale: Locale): String =
        locale.getDisplayLanguage(locale).replaceFirstChar { if (it.isLowerCase()) it.titlecase(locale) else it.toString() }
}
