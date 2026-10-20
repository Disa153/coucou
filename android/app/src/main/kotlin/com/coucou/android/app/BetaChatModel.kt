package com.coucou.android.app

import android.content.Context
import android.content.SharedPreferences
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.coucou.android.core.BetaDownAssembler
import com.coucou.android.core.BetaPageAssembler
import com.coucou.android.core.BetaPending
import com.coucou.android.link.BetaFile
import com.coucou.android.link.BetaMessage
import com.coucou.android.link.DesktopLink
import com.coucou.android.link.Protocol
import com.coucou.android.link.ServerMsg

/**
 * The beta chat of the owner and the testers, through the computer (cap `betachat`): the phone reads
 * and writes it as the GitHub account signed in on the computer. A message shows at once and is
 * replaced by the computer's copy when it has gone; photos show in the chat, videos and other files
 * open in the phone's own apps. A new message from someone else plays the chosen sound and, while
 * Coucou is not on screen, a notification. All state changes happen on the main thread.
 */
class BetaChatModel(private val context: Context, private val prefs: SharedPreferences, private val link: () -> DesktopLink?) {
    var offered by mutableStateOf(false); private set
    var messages by mutableStateOf<List<BetaMessage>>(emptyList()); private set
    var pending by mutableStateOf<List<BetaPending>>(emptyList()); private set
    var owner by mutableStateOf(false); private set
    var page by mutableStateOf(""); private set
    var error by mutableStateOf<String?>(null); private set
    var waiting by mutableStateOf(0); private set
    /** The screen is showing the chat: new messages are read, not announced. */
    var onScreen = false

    var sound by mutableStateOf(prefs.getString("beta_sound", "chime") ?: "chime"); private set
    var volume by mutableStateOf(prefs.getFloat("beta_volume", 0.8f)); private set

    /** Photos fetched, by address; videos and files go to the cache and open elsewhere. */
    val images = mutableStateMapOf<String, Bitmap>()
    val loading = mutableStateMapOf<String, Boolean>()
    private val opening = HashMap<String, BetaFile>()

    private val pages = BetaPageAssembler()
    private val downs = BetaDownAssembler()
    private val outgoing = HashMap<String, Pair<String, List<Triple<String, String, ByteArray>>>>()

    fun offer(on: Boolean) {
        offered = on
        if (on) refresh(full = true) else { waiting = 0 }
    }

    fun refresh(full: Boolean = false) { link()?.betaChatGet(full) }

    fun pickSound(kind: String) {
        if (kind !in ChatChimes.KINDS) return
        sound = kind
        prefs.edit().putString("beta_sound", kind).apply()
        ChatChimes.play(kind, volume)
    }

    fun pickVolume(v: Float) {
        volume = v.coerceIn(0f, 1f)
        prefs.edit().putFloat("beta_volume", volume).apply()
    }

    /** False when there is nothing to send or the link is down. */
    fun send(text: String, files: List<Triple<String, String, ByteArray>>): Boolean {
        val body = text.trim().take(Protocol.BETA_MAX_TEXT)
        if (body.isEmpty() && files.isEmpty()) return false
        val key = "p" + java.util.UUID.randomUUID().toString().replace("-", "").take(20)
        outgoing[key] = body to files
        pending = pending + BetaPending(key, body, files.map { it.first }, failed = false, error = null)
        if (link()?.betaChatSend(key, body, files) != true) fail(key, "network")
        return true
    }

    fun retry(key: String) {
        val (body, files) = outgoing[key] ?: return
        pending = pending.map { if (it.key == key) it.copy(failed = false, error = null) else it }
        if (link()?.betaChatSend(key, body, files) != true) fail(key, "network")
    }

    private fun fail(key: String, why: String) {
        pending = pending.map { if (it.key == key) it.copy(failed = true, error = why) else it }
    }

    fun delete(id: Long) {
        messages = messages.filterNot { it.id == id }
        link()?.betaChatDelete(id)
    }

    /** A photo is fetched to show; any other file is fetched to open. */
    fun fetch(f: BetaFile) {
        if (images.containsKey(f.url) || loading[f.url] == true) return
        loading[f.url] = true
        if (!f.mime.startsWith("image/")) opening[f.url] = f
        if (link()?.betaChatFile(f.url) != true) loading.remove(f.url)
    }

    // ── From the link (already on the main thread) ───────────────────────────────

    fun onPage(p: ServerMsg.BetaChatPage) {
        error = p.error
        if (p.error != null) return
        owner = p.owner
        page = p.page
        val all = pages.add(p) ?: return
        messages = all
        if (onScreen) waiting = 0
    }

    fun onSent(key: String, message: BetaMessage?, why: String?) {
        if (message == null) { fail(key, why ?: "network"); return }
        outgoing.remove(key)
        pending = pending.filterNot { it.key == key }
        if (messages.none { it.id == message.id }) messages = messages + message
    }

    fun onDeleted(id: Long, why: String?) {
        if (why != null) { error = why; refresh(full = true) }
    }

    fun onDown(d: ServerMsg.BetaChatDown) {
        val bytes = downs.add(d)
        if (d.error != null) { loading.remove(d.url); opening.remove(d.url); return }
        if (bytes == null) return
        loading.remove(d.url)
        val toOpen = opening.remove(d.url)
        if (toOpen == null) {
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.let { images[d.url] = it }
        } else {
            BetaFiles.open(context, toOpen, bytes)
        }
    }

    /** Someone else wrote: the sound, and a notification while Coucou is not on screen. */
    fun onNews(count: Int, login: String, text: String, inForeground: Boolean, notifier: Notifications) {
        waiting = if (onScreen) 0 else count
        ChatChimes.play(sound, volume)
        if (!inForeground) notifier.showBetaChat(login, text)
        if (onScreen) refresh()
    }

    fun onChanged() { if (onScreen) refresh() }

    fun onLinkLost() {
        offered = false
        pending = pending.map { it.copy(failed = true, error = it.error ?: "network") }
    }
}

/** Videos and files from the chat: written to this app's cache and opened with the phone's own app for them. */
object BetaFiles {
    fun open(context: Context, f: BetaFile, bytes: ByteArray) {
        runCatching {
            val dir = java.io.File(context.cacheDir, "camera").apply { mkdirs() }
            val file = java.io.File(dir, "chat-" + f.name.replace(Regex("[^A-Za-z0-9._ -]"), "_"))
            file.writeBytes(bytes)
            val uri = androidx.core.content.FileProvider.getUriForFile(context, context.packageName + ".photos", file)
            val intent = android.content.Intent(android.content.Intent.ACTION_VIEW)
                .setDataAndType(uri, f.mime)
                .addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION or android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
            context.startActivity(android.content.Intent.createChooser(intent, f.name).addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK))
        }
    }
}
