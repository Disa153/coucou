package com.coucou.android.core

import com.coucou.android.link.BetaMessage
import com.coucou.android.link.Protocol
import com.coucou.android.link.ServerMsg

/**
 * The beta chat arrives in parts (each line of the link stays under 64 KiB): the page as several
 * `betaChatPage` lines, an attachment as several `betaChatDown` lines. These put them back together.
 * Pure Kotlin, so they run in JVM unit tests.
 */
class BetaPageAssembler {
    private var parts: Array<List<BetaMessage>?> = emptyArray()

    /** The whole page once its last missing part came; null until then. A new first part starts over. */
    fun add(p: ServerMsg.BetaChatPage): List<BetaMessage>? {
        if (p.parts !in 1..64 || p.part !in 0 until p.parts) return null
        if (p.part == 0 || parts.size != p.parts) parts = arrayOfNulls(p.parts)
        parts[p.part] = p.messages
        if (parts.any { it == null }) return null
        val all = parts.flatMap { it!! }.distinctBy { it.id }.sortedBy { it.id }
        parts = emptyArray()
        return all
    }
}

/** An attachment's base64 parts, by its address; the bytes once all came. */
class BetaDownAssembler(private val maxBytes: Int = Protocol.BETA_MAX_FILE_BYTES) {
    private val files = HashMap<String, Array<String?>>()

    /** The file's bytes when this was its last missing part; null until then (or when it breaks the limits). */
    fun add(d: ServerMsg.BetaChatDown): ByteArray? {
        if (d.error != null || d.parts !in 1..400 || d.part !in 0 until d.parts) {
            files.remove(d.url)
            return null
        }
        val slots = files.getOrPut(d.url) { arrayOfNulls(d.parts) }
        if (slots.size != d.parts) {
            files.remove(d.url)
            return null
        }
        slots[d.part] = d.data
        if (slots.any { it == null }) return null
        files.remove(d.url)
        val bytes = runCatching { java.util.Base64.getDecoder().decode(slots.joinToString("")) }.getOrNull() ?: return null
        return bytes.takeIf { it.size <= maxBytes }
    }

    fun forget(url: String) { files.remove(url) }
}

/** What the beta chat screen shows: the messages, then those still on their way (with a grey tick or "Retry"). */
data class BetaPending(val key: String, val text: String, val fileNames: List<String>, val failed: Boolean, val error: String?)

object BetaChatText {
    /** "1.2 MB", "340 KB". */
    fun size(bytes: Long): String =
        if (bytes >= 1024 * 1024) String.format(java.util.Locale.US, "%.1f MB", bytes / 1024.0 / 1024.0) else "${maxOf(1, (bytes + 512) / 1024)} KB"

    /** Every message shows its writer's own colour (same table as the computer's). */
    private val COLORS = longArrayOf(0xFFF9A8D4, 0xFF93C5FD, 0xFF86EFAC, 0xFFFCD34D, 0xFFC4B5FD, 0xFFFDBA74, 0xFF5EEAD4)

    fun color(login: String): Long = COLORS[login.lowercase().sumOf { it.code } % COLORS.size]
}
