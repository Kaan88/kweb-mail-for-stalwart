package ovh.kweb.mail.ui.theme

import ovh.kweb.mail.R

/** The name of a colour choice, as the Appearance screen and the Settings row show it. */
fun themeName(choice: ThemeChoice): Int = when (choice) {
    ThemeChoice.Wallpaper -> R.string.theme_wallpaper
    ThemeChoice.Ward -> R.string.theme_ward
    ThemeChoice.Lavender -> R.string.theme_lavender
    ThemeChoice.Moss -> R.string.theme_moss
    ThemeChoice.Peach -> R.string.theme_peach
}

/** The name of a mode. */
fun themeModeName(mode: ThemeMode): Int = when (mode) {
    ThemeMode.System -> R.string.appearance_mode_system
    ThemeMode.Light -> R.string.appearance_mode_light
    ThemeMode.Dark -> R.string.appearance_mode_dark
}
