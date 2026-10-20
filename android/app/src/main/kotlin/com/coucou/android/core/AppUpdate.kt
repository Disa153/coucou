package com.coucou.android.core

import org.json.JSONObject

/**
 * Updates of this app from the fork's own releases (the all-in-one workflow publishes the APK with an
 * `android.json` beside it). The phone only reads that small file, compares numbers and, on the user's
 * tap, downloads the APK, checks it is the file the manifest names (SHA-256) and hands it to Android's
 * installer, which also refuses it unless it is signed with the same key as the app installed.
 */
data class AppRelease(val versionCode: Long, val versionName: String, val url: String, val sha256: String, val size: Long)

object AppUpdate {
    /** Where the manifest is: the fork's latest all-in-one build. */
    const val MANIFEST_URL = "https://github.com/Disa153/coucou/releases/download/all-in-one-latest/android.json"
    /** The APK may only come from the fork's releases. */
    private const val RELEASES = "https://github.com/Disa153/coucou/releases/download/"
    const val MAX_MANIFEST_BYTES = 16 * 1024
    const val MAX_APK_BYTES = 120L * 1024 * 1024
    /** Asked at most this often (the app opening, the screen turning on). */
    const val CHECK_EVERY_MS = 6L * 60 * 60 * 1000

    /** The release the manifest names, or null when it is malformed or names a file elsewhere. */
    fun parse(text: String): AppRelease? = runCatching {
        val o = JSONObject(text)
        val r = AppRelease(
            versionCode = o.getLong("versionCode"),
            versionName = o.optString("versionName", "").filter { it.isLetterOrDigit() || it in ".-+" }.take(40),
            url = o.getString("url"),
            sha256 = o.getString("sha256").lowercase(),
            size = o.optLong("size", 0),
        )
        val ok = r.versionCode > 0 && r.url.startsWith(RELEASES) && ".." !in r.url &&
            r.sha256.length == 64 && r.sha256.all { it in '0'..'9' || it in 'a'..'f' } && r.size in 0..MAX_APK_BYTES
        r.takeIf { ok }
    }.getOrNull()

    /** A release newer than what runs here. */
    fun newer(release: AppRelease?, installedCode: Long): AppRelease? = release?.takeIf { it.versionCode > installedCode }

    /** Time to ask again. */
    fun due(lastCheckMs: Long, nowMs: Long): Boolean = nowMs - lastCheckMs >= CHECK_EVERY_MS || nowMs < lastCheckMs

    fun hex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it) }
}
