package ovh.kweb.mail.background

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import uniffi.kweb_core.SecretException
import uniffi.kweb_core.SecretStore

/**
 * The core's [SecretStore]: values are sealed with AES-256-GCM under a key that never leaves
 * the AndroidKeyStore, and the ciphertext lives in a private SharedPreferences file.
 *
 * The key needs no screen unlock, so it works with the screen locked and after a boot once
 * the device has been unlocked once (the file itself is only readable then).
 *
 * The core tells "nothing stored" (null) from "cannot read right now" (an error), so every
 * failure to decrypt or to reach the Keystore is thrown as [SecretException.Failed] and only a
 * missing entry is null. The name of a value is bound to its ciphertext as associated data, so
 * ciphertexts cannot be swapped between names.
 */
class KeystoreSecrets(context: Context) : SecretStore {
    private val prefs = context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    @Synchronized
    override fun get(key: String): String? {
        val stored = prefs.getString(entry(key), null) ?: return null
        return try {
            val sealed = SealedValue.decode(stored) ?: throw IllegalStateException("stored value is damaged")
            val secretKey = existingKey() ?: throw IllegalStateException("the Keystore key is gone")
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, secretKey, GCMParameterSpec(TAG_BITS, sealed.iv))
            cipher.updateAAD(key.toByteArray())
            String(cipher.doFinal(sealed.ciphertext))
        } catch (e: Exception) {
            throw failed("read", e)
        }
    }

    @Synchronized
    override fun set(key: String, value: String) {
        val sealed = try {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, existingKey() ?: createKey())
            cipher.updateAAD(key.toByteArray())
            SealedValue(cipher.iv, cipher.doFinal(value.toByteArray()))
        } catch (e: Exception) {
            throw failed("write", e)
        }
        // commit(), not apply(): a rotated refresh token that is lost means signing in again.
        if (!prefs.edit().putString(entry(key), sealed.encode()).commit()) {
            throw SecretException.Failed("the secret could not be written to storage")
        }
    }

    @Synchronized
    override fun delete(key: String) {
        if (!prefs.edit().remove(entry(key)).commit()) {
            throw SecretException.Failed("the secret could not be removed from storage")
        }
    }

    private fun entry(key: String) = "s.$key"

    private fun failed(what: String, e: Exception) =
        SecretException.Failed("could not $what the secret: ${e.message ?: e.javaClass.simpleName}")

    private fun keyStore(): KeyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }

    private fun existingKey(): SecretKey? = keyStore().getKey(ALIAS, null) as SecretKey?

    private fun createKey(): SecretKey {
        val spec = KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setRandomizedEncryptionRequired(true)
            .setUserAuthenticationRequired(false)
            .build()
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE).run {
            init(spec)
            generateKey()
        }
    }

    private companion object {
        const val FILE = "secrets"
        const val ALIAS = "kweb_secrets_v1"
        const val ANDROID_KEYSTORE = "AndroidKeyStore"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val TAG_BITS = 128
    }
}
