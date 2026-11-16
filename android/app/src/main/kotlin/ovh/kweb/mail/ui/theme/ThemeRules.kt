package ovh.kweb.mail.ui.theme

/** Light, dark or the phone's own setting. */
enum class ThemeMode(val key: String) {
    System("system"),
    Light("light"),
    Dark("dark"),
    ;

    /** Whether the app is drawn dark, given what the phone is set to. */
    fun isDark(systemDark: Boolean): Boolean = when (this) {
        System -> systemDark
        Light -> false
        Dark -> true
    }

    companion object {
        /** The stored mode; an unknown key is the phone's setting, as before the mode existed. */
        fun of(key: String?): ThemeMode = entries.firstOrNull { it.key == key } ?: System
    }
}

/** What "Pure black" means; no Android classes here, so a JVM test checks it. */
object BlackRules {
    /** The switch can be used only where the app can be dark: not when the mode is Light. */
    fun switchEnabled(mode: ThemeMode): Boolean = mode != ThemeMode.Light

    /** Black is drawn when the user asked for it and the app is dark now (so Light with the switch on stays light). */
    fun active(pureBlack: Boolean, mode: ThemeMode, systemDark: Boolean): Boolean = pureBlack && mode.isDark(systemDark)

    // Material's guidance for OLED themes: the page is black, the layers above it step up in dark greys, so that the grouped rows
    // of the settings and the lists stay readable against it.
    const val SURFACE = 0xFF000000L
    const val CONTAINER_LOWEST = 0xFF000000L
    const val CONTAINER_LOW = 0xFF121212L
    const val CONTAINER = 0xFF1A1A1AL
    const val CONTAINER_HIGH = 0xFF232323L
    const val CONTAINER_HIGHEST = 0xFF2CL * 0x010101L + 0xFF000000L
}
