package com.harnessstudio.mobile

import android.app.Activity
import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

@InvokeArg
class SecretArgs {
    lateinit var account: String
    var value: String? = null
}

/**
 * Tokens and provider keys of the mobile client (`secrets.rs`): each value is encrypted with an
 * AES-GCM key that lives in the Android Keystore (it never leaves it) and the ciphertext is kept
 * in the app's private preferences. The page never reads a value back; Rust asks for it when it
 * signs a request.
 */
@TauriPlugin
class SecureStorePlugin(private val activity: Activity) : Plugin(activity) {
    private val prefs = activity.getSharedPreferences("harness_secrets", Context.MODE_PRIVATE)

    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build()
        )
        return generator.generateKey()
    }

    @Command
    fun get(invoke: Invoke) {
        val args = invoke.parseArgs(SecretArgs::class.java)
        val stored = prefs.getString(args.account, null)
        val result = JSObject()
        if (stored == null) {
            result.put("value", null)
            invoke.resolve(result)
            return
        }
        try {
            val bytes = Base64.decode(stored, Base64.NO_WRAP)
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes, 0, IV_SIZE))
            val plain = cipher.doFinal(bytes, IV_SIZE, bytes.size - IV_SIZE)
            result.put("value", String(plain, Charsets.UTF_8))
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("mobile.keychain")
        }
    }

    @Command
    fun set(invoke: Invoke) {
        val args = invoke.parseArgs(SecretArgs::class.java)
        val value = args.value ?: return invoke.reject("mobile.invalid_key")
        try {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, key())
            val sealed = cipher.iv + cipher.doFinal(value.toByteArray(Charsets.UTF_8))
            prefs.edit().putString(args.account, Base64.encodeToString(sealed, Base64.NO_WRAP)).apply()
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("mobile.keychain")
        }
    }

    @Command
    fun delete(invoke: Invoke) {
        val args = invoke.parseArgs(SecretArgs::class.java)
        prefs.edit().remove(args.account).apply()
        invoke.resolve()
    }

    companion object {
        private const val ALIAS = "harness_mobile_secrets"
        private const val TRANSFORMATION = "AES/GCM/NoPadding"
        private const val IV_SIZE = 12
    }
}
