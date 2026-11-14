package ovh.kweb.mail.ui

import android.app.Application
import androidx.lifecycle.HasDefaultViewModelProviderFactory
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelStore
import androidx.lifecycle.ViewModelStoreOwner
import androidx.lifecycle.viewmodel.CreationExtras
import androidx.lifecycle.viewmodel.MutableCreationExtras
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.ui.accounts.AccountViewModelFactory

/**
 * One [ViewModelStore] per route on the back stack, for screens whose state must live exactly as
 * long as the screen is on the stack: it survives a rotation (the owner of this object does) and
 * is cleared once the route is gone. Without it every visit would leave a view model behind in the
 * activity's own store.
 *
 * The owner hands out the application too: a plain `viewModel()` under it must be able to build
 * a view model that takes the [Application], as it can under the activity. Without that, a screen
 * with such a view model crashes when it opens.
 */
class ScreenStores(private val app: Application) {
    private val stores = HashMap<Route, ViewModelStore>()

    /** The account whose screens these are; a plain `viewModel()` under an owner builds view models for it. */
    var session: AccountSession? = null

    /** The store owner of [route]; the same one for as long as the route is on the stack. */
    fun owner(route: Route): ViewModelStoreOwner {
        val store = stores.getOrPut(route) { ViewModelStore() }
        return object : ViewModelStoreOwner, HasDefaultViewModelProviderFactory {
            override val viewModelStore: ViewModelStore get() = store
            override val defaultViewModelProviderFactory: ViewModelProvider.Factory
                get() = session?.let { AccountViewModelFactory(app, it) } ?: ViewModelProvider.AndroidViewModelFactory.getInstance(app)
            override val defaultViewModelCreationExtras: CreationExtras
                get() = MutableCreationExtras().apply { set(ViewModelProvider.AndroidViewModelFactory.APPLICATION_KEY, app) }
        }
    }

    /** Clears the stores of routes that are not in [stack] any more. */
    fun retain(stack: List<Route>) {
        val gone = stores.keys.filter { it !in stack }
        for (route in gone) stores.remove(route)?.clear()
    }

    fun clear() {
        stores.values.forEach { it.clear() }
        stores.clear()
    }
}
