package ovh.kweb.mail.devcal

import android.accounts.AbstractAccountAuthenticator
import android.accounts.Account
import android.accounts.AccountAuthenticatorResponse
import android.accounts.AccountManager
import android.app.Service
import android.content.ContentResolver
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.os.IBinder
import ovh.kweb.mail.MainActivity
import ovh.kweb.mail.accounts.AccountRecord

/**
 * The Android accounts of the device calendar: one per app account, of the type [DeviceCalendarRules.ACCOUNT_TYPE]. An
 * Android account holds no secret, only which app account it is for ([DeviceCalendarRules.USER_DATA_ACCOUNT]); every
 * sign-in secret stays where the app keeps it. Removing the Android account removes its calendars and events from the
 * provider.
 */
object DeviceCalendarAccounts {
    private fun manager(context: Context) = AccountManager.get(context.applicationContext)

    /** The Android account of the app account [appAccountId], if it exists. */
    fun find(context: Context, appAccountId: String): Account? {
        val manager = manager(context)
        return manager.getAccountsByType(DeviceCalendarRules.ACCOUNT_TYPE)
            .firstOrNull { manager.getUserData(it, DeviceCalendarRules.USER_DATA_ACCOUNT) == appAccountId }
    }

    /** Which app account an Android account is for (null for one that holds none). */
    fun appAccountId(context: Context, account: Account): String? =
        manager(context).getUserData(account, DeviceCalendarRules.USER_DATA_ACCOUNT)

    /**
     * The Android account of [record], made when there is none, with the calendar authority syncable, automatic and
     * asked every hour. Idempotent.
     */
    fun ensure(context: Context, record: AccountRecord, all: List<AccountRecord>): Account {
        val manager = manager(context)
        val account = find(context, record.id) ?: run {
            var made = Account(DeviceCalendarRules.accountName(record, all), DeviceCalendarRules.ACCOUNT_TYPE)
            val userData = Bundle().apply { putString(DeviceCalendarRules.USER_DATA_ACCOUNT, record.id) }
            if (!manager.addAccountExplicitly(made, null, userData)) {
                // The name is taken by an account that is not this one's: tell them apart by the app account's id.
                made = Account("${made.name} #${record.id.take(4)}", DeviceCalendarRules.ACCOUNT_TYPE)
                check(manager.addAccountExplicitly(made, null, userData)) { "the Android account could not be made" }
            }
            made
        }
        ContentResolver.setIsSyncable(account, DeviceCalendarRules.AUTHORITY, 1)
        ContentResolver.setSyncAutomatically(account, DeviceCalendarRules.AUTHORITY, true)
        ContentResolver.addPeriodicSync(account, DeviceCalendarRules.AUTHORITY, Bundle.EMPTY, DeviceCalendarRules.PERIOD_SECONDS)
        return account
    }

    /** Removes the Android account of [appAccountId]; the provider deletes its calendars and events. True when none is left. */
    fun remove(context: Context, appAccountId: String): Boolean {
        val account = find(context, appAccountId) ?: return true
        ContentResolver.removePeriodicSync(account, DeviceCalendarRules.AUTHORITY, Bundle.EMPTY)
        ContentResolver.cancelSync(account, DeviceCalendarRules.AUTHORITY)
        return manager(context).removeAccountExplicitly(account)
    }
}

/** The authenticator of the account type: it makes and keeps no secret and takes no sign-in of its own. */
class DeviceCalendarAuthenticator(private val context: Context) : AbstractAccountAuthenticator(context) {
    /** "Add account" in Android's settings: the app's own screen for it is the way, so it opens the app. */
    override fun addAccount(
        response: AccountAuthenticatorResponse?,
        accountType: String?,
        authTokenType: String?,
        requiredFeatures: Array<out String>?,
        options: Bundle?,
    ): Bundle = Bundle().apply {
        putParcelable(AccountManager.KEY_INTENT, Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    override fun editProperties(response: AccountAuthenticatorResponse?, accountType: String?): Bundle = unsupported()

    override fun confirmCredentials(response: AccountAuthenticatorResponse?, account: Account?, options: Bundle?): Bundle? = null

    override fun getAuthToken(response: AccountAuthenticatorResponse?, account: Account?, authTokenType: String?, options: Bundle?): Bundle = unsupported()

    override fun getAuthTokenLabel(authTokenType: String?): String? = null

    override fun updateCredentials(response: AccountAuthenticatorResponse?, account: Account?, authTokenType: String?, options: Bundle?): Bundle = unsupported()

    override fun hasFeatures(response: AccountAuthenticatorResponse?, account: Account?, features: Array<out String>?): Bundle =
        Bundle().apply { putBoolean(AccountManager.KEY_BOOLEAN_RESULT, false) }

    private fun unsupported() = Bundle().apply {
        putInt(AccountManager.KEY_ERROR_CODE, AccountManager.ERROR_CODE_UNSUPPORTED_OPERATION)
        putString(AccountManager.KEY_ERROR_MESSAGE, "not supported")
    }
}

/** Binds the system to the authenticator. */
class DeviceCalendarAuthenticatorService : Service() {
    private val authenticator by lazy { DeviceCalendarAuthenticator(this) }

    override fun onBind(intent: Intent?): IBinder? = authenticator.iBinder
}
