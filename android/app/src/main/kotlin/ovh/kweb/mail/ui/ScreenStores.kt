package ovh.kweb.mail.ui

import androidx.lifecycle.ViewModelStore
import androidx.lifecycle.ViewModelStoreOwner

/**
 * One [ViewModelStore] per route on the back stack, for screens whose state must live exactly as
 * long as the screen is on the stack: it survives a rotation (the owner of this object does) and
 * is cleared once the route is gone. Without it every visit would leave a view model behind in the
 * activity's own store.
 */
class ScreenStores {
    private val stores = HashMap<Route, ViewModelStore>()

    /** The store owner of [route]; the same one for as long as the route is on the stack. */
    fun owner(route: Route): ViewModelStoreOwner {
        val store = stores.getOrPut(route) { ViewModelStore() }
        return object : ViewModelStoreOwner {
            override val viewModelStore: ViewModelStore get() = store
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
