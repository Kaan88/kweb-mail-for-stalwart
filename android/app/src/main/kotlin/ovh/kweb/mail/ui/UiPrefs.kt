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

    /** The language last asked for in the assistant's Translate; null until used. */
    var translateLanguage: String?
        get() = prefs.getString(TRANSLATE, null)
        set(value) = prefs.edit { putString(TRANSLATE, value) }

    private companion object {
        const val FILE = "ui"
        const val TRANSLATE = "assistant_translate_language"
        const val THEME = "theme"
        const val ASKED = "notification_permission_asked"
    }
}
