package ovh.kweb.mail.accounts

import android.content.Context

/** [RegistryStorage] over the private prefs file `accounts`. Every write is one `commit()`, so it is on disk when it returns. */
class AccountRegistry(context: Context) : RegistryStorage {
    private val prefs = context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    override fun read(): Map<String, String>? {
        val all = prefs.all
        if (all.isEmpty()) return null
        return all.mapNotNull { (k, v) -> (v as? String)?.let { k to it } }.toMap()
    }

    override fun write(values: Map<String, String>): Boolean {
        val edit = prefs.edit().clear()
        values.forEach { (k, v) -> edit.putString(k, v) }
        return edit.commit()
    }

    private companion object {
        const val FILE = "accounts"
    }
}
