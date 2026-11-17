package ovh.kweb.mail.ui

import android.content.Context
import androidx.core.content.edit

/** Small memories of the screens (not the notification settings, which live in `Settings`). */
class UiPrefs(context: Context) {
    private val prefs = context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    /** The notification permission has been asked for, at least once. */
    var notificationPermissionAsked: Boolean
        get() = prefs.getBoolean(ASKED, false)
        set(value) = prefs.edit { putBoolean(ASKED, value) }

    /** The key of the chosen theme (`ThemeChoice.key`); null until the user picks one. */
    var theme: String?
        get() = prefs.getString(THEME, null)
        set(value) = prefs.edit { putString(THEME, value) }

    /** Light, dark or system (`ThemeMode.key`); null until the user picks one. */
    var themeMode: String?
        get() = prefs.getString(THEME_MODE, null)
        set(value) = prefs.edit { putString(THEME_MODE, value) }

    /** Dark theme with a black page. */
    var pureBlack: Boolean
        get() = prefs.getBoolean(PURE_BLACK, false)
        set(value) = prefs.edit { putBoolean(PURE_BLACK, value) }

    /** The language last asked for in the assistant's Translate; null until used. */
    var translateLanguage: String?
        get() = prefs.getString(TRANSLATE, null)
        set(value) = prefs.edit { putString(TRANSLATE, value) }

    /** Conversation view: the list groups mail by conversation (true, today's way) or shows one row per message. */
    var conversationView: Boolean
        get() = prefs.getBoolean(CONVERSATIONS, true)
        set(value) = prefs.edit { putBoolean(CONVERSATIONS, value) }

    /** When an opened message is marked read. */
    var markRead: ReadingRules.MarkRead
        get() = ReadingRules.MarkRead.of(prefs.getString(MARK_READ, null))
        set(value) = prefs.edit { putString(MARK_READ, value.key) }

    /** Where Archive or Delete in the thread screen goes next. */
    var afterAction: ReadingRules.AfterAction
        get() = ReadingRules.AfterAction.of(prefs.getString(AFTER_ACTION, null))
        set(value) = prefs.edit { putString(AFTER_ACTION, value.key) }

    /** The size of a message's text. */
    var textSize: ReadingRules.TextSize
        get() = ReadingRules.TextSize.of(prefs.getString(TEXT_SIZE, null))
        set(value) = prefs.edit { putString(TEXT_SIZE, value.key) }

    /** How many lines of a message's start the list shows under the subject (0 to 3). */
    var previewLines: Int
        get() = ReadingRules.clampPreviewLines(prefs.getInt(PREVIEW_LINES, ReadingRules.DEFAULT_PREVIEW_LINES))
        set(value) = prefs.edit { putInt(PREVIEW_LINES, ReadingRules.clampPreviewLines(value)) }

    /** What swiping a conversation right does in the mail list. */
    var swipeRight: ActionRules.Swipe
        get() = ActionRules.Swipe.of(prefs.getString(SWIPE_RIGHT, null), ActionRules.DEFAULT_RIGHT)
        set(value) = prefs.edit { putString(SWIPE_RIGHT, value.key) }

    /** What swiping a conversation left does in the mail list. */
    var swipeLeft: ActionRules.Swipe
        get() = ActionRules.Swipe.of(prefs.getString(SWIPE_LEFT, null), ActionRules.DEFAULT_LEFT)
        set(value) = prefs.edit { putString(SWIPE_LEFT, value.key) }

    /** Delete asks first (off by default). */
    var confirmDelete: Boolean
        get() = prefs.getBoolean(CONFIRM_DELETE, false)
        set(value) = prefs.edit { putBoolean(CONFIRM_DELETE, value) }

    /** How many seconds Undo of a send lasts (one of [ActionRules.UNDO_CHOICES]). */
    var undoSeconds: Int
        get() = ActionRules.undoSeconds(prefs.getInt(UNDO_SECONDS, ActionRules.DEFAULT_UNDO_SECONDS))
        set(value) = prefs.edit { putInt(UNDO_SECONDS, ActionRules.undoSeconds(value)) }

    /** The thread's primary button is Reply all when the message has more than one recipient. */
    var replyAllDefault: Boolean
        get() = prefs.getBoolean(REPLY_ALL, false)
        set(value) = prefs.edit { putBoolean(REPLY_ALL, value) }

    /** The weekly check for a newer release is on (on by default from 0.2.0). */
    var updateWeekly: Boolean
        get() = prefs.getBoolean(UPDATE_WEEKLY, true)
        set(value) = prefs.edit { putBoolean(UPDATE_WEEKLY, value) }

    /** When the update check last got an answer (milliseconds); 0 for never. */
    var updateLastChecked: Long
        get() = prefs.getLong(UPDATE_CHECKED, 0L)
        set(value) = prefs.edit { putLong(UPDATE_CHECKED, value) }

    /** The newest published version the last check saw; empty for none. */
    var updateLatest: String
        get() = prefs.getString(UPDATE_LATEST, "").orEmpty()
        set(value) = prefs.edit { putString(UPDATE_LATEST, value) }

    /** The release page the last check saw; empty for none. */
    var updatePageUrl: String
        get() = prefs.getString(UPDATE_PAGE, "").orEmpty()
        set(value) = prefs.edit { putString(UPDATE_PAGE, value) }

    /** The last check found a version newer than the installed one. */
    var updateNewer: Boolean
        get() = prefs.getBoolean(UPDATE_NEWER, false)
        set(value) = prefs.edit { putBoolean(UPDATE_NEWER, value) }

    /** The version the quiet notice was shown for, so it is not shown again for the same one. */
    var updateNotified: String
        get() = prefs.getString(UPDATE_NOTIFIED, "").orEmpty()
        set(value) = prefs.edit { putString(UPDATE_NOTIFIED, value) }

    /** Download a newer version by itself, on an unmetered network only. */
    var updateAuto: Boolean
        get() = prefs.getBoolean(UPDATE_AUTO, true)
        set(value) = prefs.edit { putBoolean(UPDATE_AUTO, value) }

    /** The newest release's file: address, size in bytes and SHA-256 (empty or 0 when it has none). */
    var updateApkUrl: String
        get() = prefs.getString(UPDATE_APK_URL, "").orEmpty()
        set(value) = prefs.edit { putString(UPDATE_APK_URL, value) }

    var updateApkSize: Long
        get() = prefs.getLong(UPDATE_APK_SIZE, 0L)
        set(value) = prefs.edit { putLong(UPDATE_APK_SIZE, value) }

    var updateApkSha: String
        get() = prefs.getString(UPDATE_APK_SHA, "").orEmpty()
        set(value) = prefs.edit { putString(UPDATE_APK_SHA, value) }

    /** What changed in the newest release, as plain text from the core; empty for none. Shown as text only. */
    var updateNotes: String
        get() = prefs.getString(UPDATE_NOTES, "").orEmpty()
        set(value) = prefs.edit { putString(UPDATE_NOTES, value) }

    /** Puts the swipes, the delete question, the Undo time and the reply choice back as they were. */
    fun resetActions() = prefs.edit {
        remove(SWIPE_RIGHT)
        remove(SWIPE_LEFT)
        remove(CONFIRM_DELETE)
        remove(UNDO_SECONDS)
        remove(REPLY_ALL)
    }

    private companion object {
        const val SWIPE_RIGHT = "actions_swipe_right"
        const val SWIPE_LEFT = "actions_swipe_left"
        const val CONFIRM_DELETE = "actions_confirm_delete"
        const val UNDO_SECONDS = "actions_undo_seconds"
        const val REPLY_ALL = "actions_reply_all"
        const val UPDATE_WEEKLY = "update_weekly"
        const val UPDATE_CHECKED = "update_last_checked"
        const val UPDATE_LATEST = "update_latest"
        const val UPDATE_PAGE = "update_page_url"
        const val UPDATE_NEWER = "update_newer"
        const val UPDATE_NOTIFIED = "update_notified"
        const val UPDATE_AUTO = "update_auto"
        const val UPDATE_APK_URL = "update_apk_url"
        const val UPDATE_APK_SIZE = "update_apk_size"
        const val UPDATE_APK_SHA = "update_apk_sha"
        const val UPDATE_NOTES = "update_notes"
        const val CONVERSATIONS = "reading_conversation_view"
        const val MARK_READ = "reading_mark_read"
        const val AFTER_ACTION = "reading_after_action"
        const val TEXT_SIZE = "reading_text_size"
        const val PREVIEW_LINES = "reading_preview_lines"
        const val FILE = "ui"
        const val TRANSLATE = "assistant_translate_language"
        const val THEME = "theme"
        const val THEME_MODE = "theme_mode"
        const val PURE_BLACK = "theme_pure_black"
        const val ASKED = "notification_permission_asked"
    }
}
