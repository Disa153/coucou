package com.coucou.android.app

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The router's password for "Turn on computer", encrypted with an AES-GCM key of its own that never leaves the Android
 * Keystore (the pairing has its own, in [SecureStore]). Backups are off, so it stays on this device.
 */
class RouterSecret(context: Context) {
    private val prefs = context.getSharedPreferences("coucou_router", Context.MODE_PRIVATE)

    private fun key(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(
                KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .setKeySize(256)
                    .build(),
            )
        }.generateKey()
    }

    /** Empty removes it. */
    fun save(password: String) {
        if (password.isEmpty()) {
            prefs.edit().remove(PREF).apply()
            return
        }
        val c = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, key()) }
        val ct = c.doFinal(password.toByteArray(Charsets.UTF_8))
        prefs.edit().putString(PREF, Base64.encodeToString(c.iv + ct, Base64.NO_WRAP)).apply()
    }

    fun load(): String {
        val blob = prefs.getString(PREF, null) ?: return ""
        return try {
            val raw = Base64.decode(blob, Base64.NO_WRAP)
            val c = Cipher.getInstance("AES/GCM/NoPadding").apply {
                init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, raw, 0, IV_BYTES))
            }
            String(c.doFinal(raw, IV_BYTES, raw.size - IV_BYTES), Charsets.UTF_8)
        } catch (_: Exception) {
            ""
        }
    }

    private companion object {
        const val ALIAS = "coucou_router_key"
        const val PREF = "password"
        const val IV_BYTES = 12
    }
}
