package ovh.kweb.mail.ui.accounts

import android.app.Application
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.lifecycle.HasDefaultViewModelProviderFactory
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelStore
import androidx.lifecycle.ViewModelStoreOwner
import androidx.lifecycle.viewmodel.CreationExtras
import ovh.kweb.mail.accounts.AccountSession

/** The account whose screens are on display. Provided by `AppContent` around the signed-in screens; they must not be shown without it. */
val LocalAccountSession = staticCompositionLocalOf<AccountSession> { error("no account session provided") }

/**
 * Builds a view model that works for one account: it is given the [AccountSession] and keeps it, so a later
 * switch of the account cannot send what it does to another account's Core. It builds view models with a
 * constructor `(Application, AccountSession)`; anything else falls back to the Android factory.
 */
class AccountViewModelFactory(private val app: Application, private val session: AccountSession) : ViewModelProvider.Factory {
    override fun <T : ViewModel> create(modelClass: Class<T>, extras: CreationExtras): T {
        val constructor = modelClass.constructors.firstOrNull {
            it.parameterTypes.contentEquals(arrayOf(Application::class.java, AccountSession::class.java))
        } ?: return ViewModelProvider.AndroidViewModelFactory.getInstance(app).create(modelClass, extras)
        @Suppress("UNCHECKED_CAST")
        return constructor.newInstance(app, session) as T
    }
}

/** A store of view models owned by one account: it is cleared when the account is switched or removed. */
class AccountStoreOwner(private val app: Application, session: AccountSession, private val store: ViewModelStore) :
    ViewModelStoreOwner, HasDefaultViewModelProviderFactory {
    override val viewModelStore: ViewModelStore get() = store
    override val defaultViewModelProviderFactory: ViewModelProvider.Factory = AccountViewModelFactory(app, session)
    override val defaultViewModelCreationExtras: CreationExtras
        get() = androidx.lifecycle.viewmodel.MutableCreationExtras().apply { set(ViewModelProvider.AndroidViewModelFactory.APPLICATION_KEY, app) }
}
