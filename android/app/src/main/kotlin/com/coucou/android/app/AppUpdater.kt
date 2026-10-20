package com.coucou.android.app

import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.FileProvider
import com.coucou.android.core.AppRelease
import com.coucou.android.core.AppUpdate
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import kotlin.concurrent.thread

/**
 * "Update available": reads the fork's `android.json` now and then (only to compare numbers; nothing
 * about the user is sent), and on the user's tap downloads the APK into the cache, checks its SHA-256
 * and opens Android's installer on it. Android itself asks once to allow installs from Coucou.
 */
class AppUpdater(private val context: Context, private val prefs: SharedPreferences) {
    enum class Phase { IDLE, DOWNLOADING, VERIFYING, READY, FAILED }

    var available by mutableStateOf<AppRelease?>(null); private set
    var phase by mutableStateOf(Phase.IDLE); private set
    var progress by mutableStateOf(0f); private set
    /** The user closed the banner for this build. */
    var dismissed by mutableStateOf(prefs.getLong("update_dismissed", 0L)); private set

    private val main = Handler(Looper.getMainLooper())
    @Volatile private var checking = false

    val installedCode: Long
        get() = runCatching {
            val info = context.packageManager.getPackageInfo(context.packageName, 0)
            if (android.os.Build.VERSION.SDK_INT >= 28) info.longVersionCode else @Suppress("DEPRECATION") info.versionCode.toLong()
        }.getOrDefault(Long.MAX_VALUE)

    val installedName: String
        get() = runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName.orEmpty() }.getOrDefault("")

    /** The banner is up: a newer build than this one, not put away. */
    val showBanner: Boolean get() = available.let { it != null && it.versionCode != dismissed }

    /** On the app opening: asks GitHub if it is time to. [force] from Settings → Check for updates. */
    fun check(force: Boolean = false, done: ((Boolean) -> Unit)? = null) {
        val now = System.currentTimeMillis()
        if (checking || (!force && !AppUpdate.due(prefs.getLong("update_checked", 0L), now))) { done?.invoke(false); return }
        checking = true
        if (force) { dismissed = 0L; prefs.edit().remove("update_dismissed").apply() } // asked by hand: show it again
        thread(name = "coucou-update-check", isDaemon = true) {
            val release = runCatching { AppUpdate.parse(String(get(AppUpdate.MANIFEST_URL, AppUpdate.MAX_MANIFEST_BYTES.toLong()), Charsets.UTF_8)) }.getOrNull()
            main.post {
                checking = false
                prefs.edit().putLong("update_checked", now).apply()
                available = AppUpdate.newer(release, installedCode)
                done?.invoke(release != null)
            }
        }
    }

    fun dismiss() {
        val code = available?.versionCode ?: return
        dismissed = code
        prefs.edit().putLong("update_dismissed", code).apply()
    }

    /** Downloads, checks and opens the installer; asks for the right to install first if Android needs it. */
    fun install() {
        val r = available ?: return
        if (phase == Phase.DOWNLOADING || phase == Phase.VERIFYING) return
        if (android.os.Build.VERSION.SDK_INT >= 26 && !context.packageManager.canRequestPackageInstalls()) {
            runCatching {
                context.startActivity(
                    Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${context.packageName}")).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                )
            }
            return
        }
        phase = Phase.DOWNLOADING
        progress = 0f
        thread(name = "coucou-update-download", isDaemon = true) {
            val dir = File(context.cacheDir, "updates").apply { mkdirs() }
            dir.listFiles()?.forEach { it.delete() }
            val file = File(dir, "Coucou-${r.versionCode}.apk")
            val ok = runCatching {
                download(r.url, file, AppUpdate.MAX_APK_BYTES) { p -> main.post { progress = p } }
                main.post { phase = Phase.VERIFYING }
                val digest = MessageDigest.getInstance("SHA-256")
                file.inputStream().use { input ->
                    val buf = ByteArray(64 * 1024)
                    while (true) { val n = input.read(buf); if (n < 0) break; digest.update(buf, 0, n) }
                }
                AppUpdate.hex(digest.digest()) == r.sha256
            }.getOrDefault(false)
            main.post {
                if (!ok) { file.delete(); phase = Phase.FAILED; return@post }
                phase = Phase.READY
                openInstaller(file)
            }
        }
    }

    fun openInstaller(file: File? = File(context.cacheDir, "updates").listFiles()?.firstOrNull()) {
        file ?: return
        runCatching {
            val uri = FileProvider.getUriForFile(context, context.packageName + ".photos", file)
            context.startActivity(
                Intent(Intent.ACTION_VIEW).setDataAndType(uri, "application/vnd.android.package-archive")
                    .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK),
            )
        }.onFailure { phase = Phase.FAILED }
    }

    private fun open(url: String): HttpURLConnection {
        val c = URL(url).openConnection() as HttpURLConnection
        c.connectTimeout = 15_000
        c.readTimeout = 30_000
        c.instanceFollowRedirects = true
        c.setRequestProperty("User-Agent", "Coucou-Android")
        return c
    }

    private fun get(url: String, max: Long): ByteArray {
        val c = open(url)
        try {
            if (c.responseCode != 200) throw java.io.IOException("HTTP ${c.responseCode}")
            return c.inputStream.use { input ->
                val out = java.io.ByteArrayOutputStream()
                val buf = ByteArray(8 * 1024)
                while (true) {
                    val n = input.read(buf)
                    if (n < 0) break
                    out.write(buf, 0, n)
                    if (out.size() > max) throw java.io.IOException("too large")
                }
                out.toByteArray()
            }
        } finally { c.disconnect() }
    }

    private fun download(url: String, to: File, max: Long, onProgress: (Float) -> Unit) {
        val c = open(url)
        try {
            if (c.responseCode != 200) throw java.io.IOException("HTTP ${c.responseCode}")
            val total = c.contentLengthLong.takeIf { it > 0 } ?: available?.size ?: 0L
            var got = 0L
            c.inputStream.use { input ->
                to.outputStream().use { out ->
                    val buf = ByteArray(64 * 1024)
                    var last = 0L
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        out.write(buf, 0, n)
                        got += n
                        if (got > max) throw java.io.IOException("too large")
                        if (total > 0 && got - last > 256 * 1024) { last = got; onProgress(got.toFloat() / total) }
                    }
                }
            }
        } finally { c.disconnect() }
    }
}
