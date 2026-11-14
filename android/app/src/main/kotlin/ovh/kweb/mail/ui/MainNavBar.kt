package ovh.kweb.mail.ui

import androidx.annotation.DrawableRes
import androidx.annotation.StringRes
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import ovh.kweb.mail.R

/** One entry of the bottom bar: the root screen it leads to. */
data class MainTab(val route: Route, @StringRes val label: Int, @DrawableRes val icon: Int)

/** The entries of the bottom bar, in order. */
object MainTabs {
    val all = listOf(
        MainTab(Route.MailList, R.string.tab_mail, R.drawable.ic_mail),
        MainTab(Route.Calendar, R.string.tab_calendar, R.drawable.ic_calendar),
        MainTab(Route.Contacts, R.string.tab_contacts, R.drawable.ic_person),
        MainTab(Route.Files, R.string.tab_files, R.drawable.ic_files),
    )
}

/**
 * The tonal bottom bar of the Inbox, Calendar, Contacts and Files figures. [selected] is the route on
 * screen; [mailUnread] is the count on the Mail entry, the Inbox's unread mail.
 */
@Composable
fun MainNavBar(selected: Route, mailUnread: Int, onSelect: (Route) -> Unit) {
    NavigationBar(containerColor = MaterialTheme.colorScheme.surfaceContainer) {
        for (tab in MainTabs.all) {
            val label = stringResource(tab.label)
            val on = tab.route == selected
            NavigationBarItem(
                selected = on,
                onClick = { onSelect(tab.route) },
                icon = {
                    val badge = tab.route == Route.MailList && mailUnread > 0
                    if (badge) {
                        val description = pluralStringResource(R.plurals.unread_count, mailUnread, mailUnread)
                        BadgedBox(
                            badge = {
                                Badge { Text(if (mailUnread > 99) "99+" else mailUnread.toString()) }
                            },
                            modifier = Modifier.semantics { contentDescription = "$label, $description" },
                        ) { Icon(painterResource(tab.icon), contentDescription = null) }
                    } else {
                        Icon(painterResource(tab.icon), contentDescription = null)
                    }
                },
                label = { Text(label, fontWeight = if (on) FontWeight.Bold else FontWeight.Normal) },
            )
        }
    }
}
