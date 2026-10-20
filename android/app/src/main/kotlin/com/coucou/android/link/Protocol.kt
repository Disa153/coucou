package com.coucou.android.link

import com.coucou.android.mochi.BotState
import com.coucou.android.mochi.outfit.Wardrobe
import java.security.MessageDigest
import org.json.JSONArray
import org.json.JSONException
import org.json.JSONObject

/**
 * Wire format of the desktop link (docs/ANDROID_LINK.md): newline-delimited JSON over TLS.
 * Pure Kotlin plus org.json, so it runs in JVM unit tests.
 */
object Protocol {
    const val VERSION = 1
    /** The desktop dismisses an approval after 115 s; the phone stops offering it at 120 s. */
    const val APPROVAL_TTL_MS = 120_000L
    /** One message never exceeds this; a longer line is a protocol error. */
    const val MAX_LINE_BYTES = 64 * 1024
    /** Optional features this app understands; the desktop offers back the ones it has switched on. */
    val CAPABILITIES = listOf("chat", "details", "answers", "prefs", "diffs", "usage", "services", "relay", "wake", "betachat")
    const val CAP_CHAT = "chat"
    /** Steps, last line, project folder name and colour of each session. */
    const val CAP_DETAILS = "details"
    /** Answering the questions Claude Code asks, from the phone (each answer confirmed with the screen lock). */
    const val CAP_ANSWERS = "answers"
    /** What Mochi wears on the computer (no switch there: it is no secret). */
    const val CAP_PREFS = "prefs"
    /** The files an agent changed (a list on each session) and, when asked for one, its lines. */
    const val CAP_DIFFS = "diffs"
    /** How much of the Claude and Codex plans is used, and when they reset. */
    const val CAP_USAGE = "usage"
    /** Read-only cards of the services the user ticked on the computer (Stripe, GitHub...). */
    const val CAP_SERVICES = "services"
    /** The computer's relay switch is on: it sends the new access key when the user replaces it (docs/RELAY_LINK.md). */
    const val CAP_RELAY = "relay"
    /** This computer's wired card and network, kept for "Turn on computer" (Wake-on-LAN) when it is off. */
    const val CAP_WAKE = "wake"
    /** The beta chat of the owner and the testers, read and written as the computer's GitHub account (docs/ANDROID_LINK.md). */
    const val CAP_BETA_CHAT = "betachat"
    /** As on the computer (betachat.rs): at most this many attachments of this size in one message. */
    const val BETA_MAX_FILES = 4
    const val BETA_MAX_FILE_BYTES = 7 * 1024 * 1024
    /** Raw bytes in one part of a file sent to the computer (40 000 base64 characters). */
    const val BETA_PART_BYTES = 30_000
    const val BETA_MAX_TEXT = 2000
    /** Photos in the chat: at most this many per message, each a JPEG cut in parts of [CHAT_IMAGE_PART_BYTES]. */
    const val CHAT_MAX_IMAGES = 4
    const val CHAT_IMAGE_PART_BYTES = 36_000
    const val CHAT_MAX_IMAGE_BYTES = 4 * 1024 * 1024
    const val MAX_SERVICES = 7
    const val MAX_SERVICE_ITEMS = 3
    const val MAX_SERVICE_TEXT = 80
    const val MAX_FILES = 20
    const val MAX_FILE_NAME_CHARS = 80
    /** The computer sends at most this many lines of one file, each cut at [MAX_DIFF_LINE_CHARS], in parts of at most [PART_LINES]. */
    const val MAX_DIFF_LINES = 200
    const val MAX_DIFF_LINE_CHARS = 400
    const val PART_LINES = 100
    const val MAX_DIFF_PARTS = 4
    const val MAX_QUESTIONS = 4
    const val MAX_OPTIONS = 8
    const val MAX_STEPS = 20
    const val MAX_STEP_CHARS = 200
    const val MAX_PROJECT_CHARS = 64
    /** What the desktop accepts for one chat message; longer is refused there, so it is refused here first. */
    const val CHAT_MAX_TEXT = 4000
}

/** A model the user allowed on the computer for the phone. [id] is "provider/model". */
data class ChatModel(val id: String, val provider: String, val label: String)

/** One window of a plan: whole percent used (0..100) and when it resets (epoch ms). */
data class PlanWindow(val pct: Int, val resetsAtMs: Long)

/** What the computer knows of one plan. [plan] is a short name such as "plus"; [resetCredits] Codex's free resets. */
data class PlanUsage(
    val fiveHour: PlanWindow?, val sevenDay: PlanWindow?, val resetCredits: Int? = null, val plan: String? = null, val updatedAtMs: Long = 0,
)

/** Both plans; null for one the computer does not know. */
data class UsageSnapshot(val claude: PlanUsage?, val codex: PlanUsage?)

/** One line of a service card. */
data class ServiceLine(val label: String, val detail: String)

/** What one service's pill shows: a headline, why, up to three lines. Read-only. */
data class ServiceCard(val id: String, val title: String, val headline: String, val reason: String?, val items: List<ServiceLine>)

/** A file an agent changed. [name] is a file name, never a path. */
data class FileChange(val id: Long, val name: String, val added: Int, val removed: Int, val tooLarge: Boolean = false, val isNew: Boolean = false)

/** One line of a diff: [kind] is '+', '-', ' ' (context) or '@' (a hunk starts). */
data class DiffRow(val kind: Char, val text: String)

data class SessionInfo(
    val pillId: String,
    val agent: String,
    val state: BotState,
    val statusText: String,
    val stepIndex: Int,
    val stepCount: Int,
    val updatedAtMs: Long,
    // Only with the "details" capability (the user's switch on the computer); empty or null otherwise.
    val steps: List<String> = emptyList(),
    val finalLine: String? = null,
    /** A folder name, never a path. */
    val project: String? = null,
    /** "#RRGGBB" or null. */
    val color: String? = null,
    /** Only with the "diffs" capability (the user's switch on the computer). */
    val files: List<FileChange> = emptyList(),
)

data class ApprovalRequest(
    val pillId: String,
    val fingerprint: String,
    val tool: String,
    val command: String,
    val createdAtMs: Long,
    /** The computer cut the command to fit (it sends at most 500 characters): what is shown is not all of it. Additive; absent means false. */
    val truncated: Boolean = false,
)

/** One option of a question Claude Code asked. */
data class AskedOption(val label: String, val description: String)

/** One question of an AskUserQuestion call. */
data class AskedQuestion(val question: String, val options: List<AskedOption>, val multiSelect: Boolean)

/** A question waiting for an answer (cap `answers`). */
data class QuestionRequest(val pillId: String, val fingerprint: String, val questions: List<AskedQuestion>, val createdAtMs: Long)

/** An attachment of a beta chat message: a file the computer fetches from the sender's Gist at [url] when asked. */
data class BetaFile(val name: String, val mime: String, val size: Long, val url: String)

/** One message of the beta chat; [mine]: written by the GitHub account signed in on the computer. */
data class BetaMessage(
    val id: Long, val login: String, val text: String, val build: Long?, val files: List<BetaFile>,
    val atSec: Long, val owner: Boolean, val mine: Boolean,
)

/** What the computer said about itself for Wake-on-LAN: its card's MAC ("aa:bb:..."), its address and its network's broadcast address. */
data class WakeTarget(val mac: String, val ip: String, val broadcast: String)

sealed interface ServerMsg {
    data class Welcome(
        val version: Int, val desktopName: String, val os: String, val caps: Set<String> = emptySet(),
        /** Only to a phone that asked for `wake`, when the computer could read its card. */
        val wake: WakeTarget? = null,
    ) : ServerMsg
    data class Sessions(val sessions: List<SessionInfo>) : ServerMsg
    data class Approval(val request: ApprovalRequest) : ServerMsg
    data class ApprovalResolved(val fingerprint: String) : ServerMsg
    data class Question(val request: QuestionRequest) : ServerMsg
    /** What Mochi wears on the computer: "auto" or an outfit, exactly one of [com.coucou.android.mochi.outfit.Wardrobe.SELECTIONS]. */
    data class Prefs(val outfit: String) : ServerMsg
    /** The relay's access key was replaced on the computer (only with the `relay` capability). */
    data class RelayAccess(val access: String) : ServerMsg
    /** The service cards the user allowed; an empty list takes them away. */
    data class Services(val cards: List<ServiceCard>) : ServerMsg
    /** The plan usage; both null means the computer has nothing to say any more. */
    data class Usage(val usage: UsageSnapshot) : ServerMsg
    /** One part of a file's diff; [gone]: the computer no longer has it; [truncated]: there were more lines than it sends. */
    data class Diff(
        val pillId: String, val fileId: Long, val name: String, val added: Int, val removed: Int,
        val tooLarge: Boolean, val gone: Boolean, val truncated: Boolean, val part: Int, val parts: Int, val lines: List<DiffRow>,
    ) : ServerMsg
    data object Pong : ServerMsg
    data class ChatModels(val models: List<ChatModel>) : ServerMsg
    /** [text] is appended to the answer being written. */
    data class ChatDelta(val id: String, val text: String) : ServerMsg
    /** [text], when present, is the full answer and replaces what was streamed. */
    data class ChatDone(val id: String, val text: String?) : ServerMsg
    /** [reason] is a fixed code (off, busy, rate, auth...); [message] a fixed sentence written by the computer. */
    data class ChatError(val id: String, val reason: String, val message: String) : ServerMsg
    data class Error(val code: String, val message: String) : ServerMsg
    /** One part of the beta chat; [error] instead when it could not be read. */
    data class BetaChatPage(val part: Int, val parts: Int, val owner: Boolean, val page: String, val messages: List<BetaMessage>, val error: String?) : ServerMsg
    data class BetaChatSent(val key: String, val message: BetaMessage?, val error: String?) : ServerMsg
    data class BetaChatDeleted(val id: Long, val error: String?) : ServerMsg
    /** One part of an attachment's bytes (base64), or why it could not be fetched. */
    data class BetaChatDown(val url: String, val part: Int, val parts: Int, val data: String, val error: String?) : ServerMsg
    /** Someone else wrote in the beta chat. */
    data class BetaChatNews(val unread: Int, val login: String, val text: String) : ServerMsg
    data object BetaChatChanged : ServerMsg
}

sealed interface ClientMsg {
    data class Hello(val version: Int, val token: String, val deviceName: String, val caps: List<String> = emptyList()) : ClientMsg
    /** Only allow and deny exist from the phone: no "always". */
    data class Decision(val fingerprint: String, val allow: Boolean) : ClientMsg
    data object Ping : ClientMsg
    data object Bye : ClientMsg
    /** [picks]: one list of labels per question, in order (exactly one for a single choice). */
    data class Answer(val fingerprint: String, val picks: List<List<String>>) : ClientMsg
    /** The lines of one file the session list showed (cap `diffs`). */
    data class GetDiff(val pillId: String, val fileId: Long) : ClientMsg
    data object ChatModels : ClientMsg
    /** [images]: the ids of the photos sent just before as [ChatImage] lines (none: a text message, as before). */
    data class ChatSend(val id: String, val model: String, val text: String, val images: List<String> = emptyList()) : ClientMsg
    /** One part of a photo for the next [ChatSend]: base64 JPEG [data], part [part] of [parts]. */
    data class ChatImage(val id: String, val part: Int, val parts: Int, val data: String) : ClientMsg
    data class ChatCancel(val id: String) : ClientMsg
    /** New chat: the computer forgets its side of the conversation and stops a running answer. */
    data object ChatReset : ClientMsg
    data class BetaChatGet(val full: Boolean) : ClientMsg
    /** One part of a file for the next [BetaChatSend]: base64 [data], part [part] of [parts]. */
    data class BetaChatUp(val fid: String, val part: Int, val parts: Int, val data: String) : ClientMsg
    data class BetaChatSend(val key: String, val text: String, val files: List<BetaOutFile>) : ClientMsg
    data class BetaChatDelete(val id: Long) : ClientMsg
    data class BetaChatFile(val url: String) : ClientMsg
}

/** A file sent just before as [ClientMsg.BetaChatUp] parts under [fid]. */
data class BetaOutFile(val fid: String, val name: String, val mime: String)

object Wire {
    fun encode(m: ClientMsg): String {
        val o = JSONObject()
        when (m) {
            is ClientMsg.Hello -> {
                o.put("type", "hello").put("v", m.version).put("token", m.token).put("device", m.deviceName)
                // Only when there is something to say: an app with no optional features sends the v1 hello unchanged.
                if (m.caps.isNotEmpty()) o.put("caps", JSONArray(m.caps))
            }
            is ClientMsg.Decision -> o.put("type", "decision").put("fingerprint", m.fingerprint)
                .put("decision", if (m.allow) "allow" else "deny")
            ClientMsg.Ping -> o.put("type", "ping")
            ClientMsg.Bye -> o.put("type", "bye")
            is ClientMsg.Answer -> o.put("type", "answer").put("fingerprint", m.fingerprint)
                .put("picks", JSONArray(m.picks.map { JSONArray(it) }))
            is ClientMsg.GetDiff -> o.put("type", "getDiff").put("pillId", m.pillId).put("fileId", m.fileId)
            ClientMsg.ChatModels -> o.put("type", "chatModels")
            is ClientMsg.ChatSend -> {
                o.put("type", "chatSend").put("id", m.id).put("model", m.model).put("text", m.text)
                if (m.images.isNotEmpty()) o.put("images", JSONArray(m.images))
            }
            is ClientMsg.ChatImage -> o.put("type", "chatImage").put("id", m.id).put("part", m.part).put("parts", m.parts).put("data", m.data)
            is ClientMsg.ChatCancel -> o.put("type", "chatCancel").put("id", m.id)
            ClientMsg.ChatReset -> o.put("type", "chatReset")
            is ClientMsg.BetaChatGet -> o.put("type", "betaChatGet").put("full", m.full)
            is ClientMsg.BetaChatUp -> o.put("type", "betaChatUp").put("fid", m.fid).put("part", m.part).put("parts", m.parts).put("data", m.data)
            is ClientMsg.BetaChatSend -> o.put("type", "betaChatSend").put("key", m.key).put("text", m.text)
                .put("files", JSONArray(m.files.map { JSONObject().put("fid", it.fid).put("name", it.name).put("mime", it.mime) }))
            is ClientMsg.BetaChatDelete -> o.put("type", "betaChatDelete").put("id", m.id)
            is ClientMsg.BetaChatFile -> o.put("type", "betaChatFile").put("url", m.url)
        }
        return o.toString()
    }

    /** Returns null for anything malformed or unknown: a bad line never crashes the client. */
    fun decodeServer(line: String): ServerMsg? {
        if (line.length > Protocol.MAX_LINE_BYTES) return null
        return try {
            val o = JSONObject(line)
            when (o.getString("type")) {
                "welcome" -> ServerMsg.Welcome(
                    o.getInt("v"), o.optString("desktop", ""), o.optString("os", ""), strings(o.optJSONArray("caps")),
                    o.optJSONObject("wake")?.let { w ->
                        WakeTarget(w.optString("mac", ""), w.optString("ip", ""), w.optString("broadcast", ""))
                            .takeIf { PowerOn.macBytes(it.mac) != null }
                    },
                )
                "sessions" -> ServerMsg.Sessions(o.getJSONArray("sessions").objects().map(::session))
                "approval" -> ServerMsg.Approval(
                    ApprovalRequest(
                        pillId = o.getString("pillId"),
                        fingerprint = o.getString("fingerprint").also { require(isFingerprint(it)) },
                        tool = o.getString("tool"),
                        command = o.getString("command"),
                        createdAtMs = o.getLong("createdAt"),
                        truncated = o.optBoolean("truncated", false),
                    ),
                )
                "approvalResolved" -> ServerMsg.ApprovalResolved(o.getString("fingerprint"))
                "question" -> question(o)?.let { ServerMsg.Question(it) }
                // A value this build does not know is dropped rather than guessed: the phone keeps what it had.
                "diff" -> diff(o)
                "services" -> ServerMsg.Services(services(o.getJSONArray("services")))
                "usage" -> ServerMsg.Usage(UsageSnapshot(plan(o.optJSONObject("claude")), plan(o.optJSONObject("codex"))))
                "prefs" -> o.optString("outfit", "").takeIf { it in Wardrobe.SELECTIONS }?.let { ServerMsg.Prefs(it) }
                "relayAccess" -> o.optString("access", "").takeIf(RelayPairing::isSecret)?.let { ServerMsg.RelayAccess(it) }
                "pong" -> ServerMsg.Pong
                "chatModels" -> ServerMsg.ChatModels(o.getJSONArray("models").objects().mapNotNull(::chatModel))
                "chatDelta" -> ServerMsg.ChatDelta(chatId(o), o.getString("text"))
                "chatDone" -> ServerMsg.ChatDone(chatId(o), if (o.has("text")) o.getString("text") else null)
                "chatError" -> ServerMsg.ChatError(chatId(o), o.optString("reason", "internal"), o.optString("message", ""))
                "error" -> ServerMsg.Error(o.optString("code", ""), o.optString("message", ""))
                "betaChatPage" -> ServerMsg.BetaChatPage(
                    o.optInt("part", 0), o.optInt("parts", 1).coerceIn(1, 64), o.optBoolean("owner", false),
                    o.optString("page", "").takeIf { it.startsWith("https://github.com/") }.orEmpty(),
                    o.optJSONArray("messages")?.objects()?.mapNotNull(::betaMessage).orEmpty(), errorOf(o),
                )
                "betaChatSent" -> ServerMsg.BetaChatSent(o.getString("key"), o.optJSONObject("message")?.let(::betaMessage), errorOf(o))
                "betaChatDeleted" -> ServerMsg.BetaChatDeleted(o.getLong("id"), errorOf(o))
                "betaChatDown" -> ServerMsg.BetaChatDown(
                    o.getString("url"), o.optInt("part", 0), o.optInt("parts", 1), o.optString("data", ""), errorOf(o),
                )
                "betaChatNews" -> ServerMsg.BetaChatNews(
                    o.optInt("unread", 0).coerceAtLeast(0), o.optString("login", "").take(39),
                    o.optString("text", "").filter { !it.isISOControl() || it == '\n' }.take(200),
                )
                "betaChatChanged" -> ServerMsg.BetaChatChanged
                else -> null
            }
        } catch (_: JSONException) {
            null
        } catch (_: IllegalArgumentException) {
            null
        }
    }

    private fun errorOf(o: JSONObject): String? = o.opt("error")?.let { if (it is String && it.isNotBlank()) it.take(40) else null }

    /** A beta chat message, or null if it lacks an id; its files only from a Gist (the computer checks it too). */
    fun betaMessage(o: JSONObject): BetaMessage? {
        val id = o.optLong("id", -1).takeIf { it > 0 } ?: return null
        val files = o.optJSONArray("files")?.objects()?.mapNotNull { f ->
            val url = f.optString("url", "")
            if (!url.startsWith("https://gist.githubusercontent.com/") || ".." in url) return@mapNotNull null
            BetaFile(fileNameOf(f.optString("name", "")) ?: "file", f.optString("mime", "application/octet-stream").take(80), f.optLong("size", 0).coerceAtLeast(0), url)
        }.orEmpty().take(Protocol.BETA_MAX_FILES)
        return BetaMessage(
            id = id, login = o.optString("login", "").take(39), text = o.optString("text", "").take(Protocol.BETA_MAX_TEXT),
            build = if (o.isNull("build")) null else o.optLong("build", 0).takeIf { it > 0 }, files = files,
            atSec = o.optLong("at", 0), owner = o.optBoolean("owner", false), mine = o.optBoolean("mine", false),
        )
    }

    private fun session(o: JSONObject) = SessionInfo(
        pillId = o.getString("pillId"),
        agent = o.optString("agent", ""),
        state = BotState.entries.firstOrNull { it.key == o.optString("state") } ?: BotState.IDLE,
        statusText = o.optString("statusText", ""),
        stepIndex = o.optInt("stepIndex", 0),
        stepCount = o.optInt("stepCount", 0),
        updatedAtMs = o.optLong("updatedAt", 0),
        steps = stringList(o.optJSONArray("steps")).take(Protocol.MAX_STEPS).map { it.take(Protocol.MAX_STEP_CHARS) },
        finalLine = o.optString("finalLine", "").take(Protocol.MAX_STEP_CHARS).ifBlank { null },
        project = folderName(o.optString("project", "")),
        color = o.optString("color", "").takeIf { isColor(it) },
        files = files(o.optJSONArray("files")),
    )

    private val SERVICE_IDS = setOf(
        "integration_stripe", "integration_github", "integration_vercel", "integration_n8n",
        "integration_resend", "integration_notion", "integration_calcom",
    )

    private fun oneLine(s: String, max: Int) = s.filter { !it.isISOControl() }.trim().take(max)

    /** Cards of the services this app knows, each cut to size; anything else is skipped. */
    private fun services(a: JSONArray): List<ServiceCard> {
        val seen = HashSet<String>()
        return (0 until a.length()).mapNotNull { i ->
            val o = a.optJSONObject(i) ?: return@mapNotNull null
            val id = o.optString("id", "")
            if (id !in SERVICE_IDS || !seen.add(id)) return@mapNotNull null
            val items = o.optJSONArray("items")?.let { arr ->
                (0 until arr.length()).mapNotNull { j ->
                    val it = arr.optJSONObject(j) ?: return@mapNotNull null
                    val label = oneLine(it.optString("label", ""), Protocol.MAX_SERVICE_TEXT)
                    if (label.isEmpty()) null else ServiceLine(label, oneLine(it.optString("detail", ""), Protocol.MAX_SERVICE_TEXT))
                }.take(Protocol.MAX_SERVICE_ITEMS)
            }.orEmpty()
            ServiceCard(
                id, oneLine(o.optString("title", ""), Protocol.MAX_SERVICE_TEXT), oneLine(o.optString("headline", ""), Protocol.MAX_SERVICE_TEXT),
                oneLine(o.optString("reason", ""), Protocol.MAX_SERVICE_TEXT).ifBlank { null }, items,
            )
        }.take(Protocol.MAX_SERVICES)
    }

    private fun window(o: JSONObject?): PlanWindow? {
        if (o == null || !o.has("pct") || !o.has("resetsAt")) return null
        val pct = o.optInt("pct", -1)
        val at = o.optLong("resetsAt", 0)
        return if (pct in 0..100 && at > 0) PlanWindow(pct, at) else null
    }

    private fun plan(o: JSONObject?): PlanUsage? {
        if (o == null) return null
        val five = window(o.optJSONObject("fiveHour"))
        val seven = window(o.optJSONObject("sevenDay"))
        if (five == null && seven == null) return null
        val name = o.optString("plan", "").filter { it.isLetterOrDigit() || it == ' ' || it == '-' }.trim().take(20).ifBlank { null }
        val credits = if (o.has("resetCredits")) o.optInt("resetCredits", -1).takeIf { it in 0..99 } else null
        return PlanUsage(five, seven, credits, name, o.optLong("updatedAt", 0).coerceAtLeast(0))
    }

    /** Files with an id and a name, as names only (even if a path were sent), at most [Protocol.MAX_FILES]. */
    private fun files(a: JSONArray?): List<FileChange> {
        if (a == null) return emptyList()
        return (0 until a.length()).mapNotNull { i ->
            val f = a.optJSONObject(i) ?: return@mapNotNull null
            if (!f.has("id")) return@mapNotNull null
            val id = f.optLong("id", -1)
            val name = fileNameOf(f.optString("name", "")) ?: return@mapNotNull null
            if (id < 0) return@mapNotNull null
            FileChange(id, name, f.optInt("added", 0).coerceAtLeast(0), f.optInt("removed", 0).coerceAtLeast(0), f.optBoolean("tooLarge", false), f.optBoolean("isNew", false))
        }.takeLast(Protocol.MAX_FILES)
    }

    /** The last segment of a path, without control characters and cut; null if nothing is left. */
    fun fileNameOf(raw: String): String? {
        val last = raw.trimEnd('/', '\\').substringAfterLast('/').substringAfterLast('\\').filter { !it.isISOControl() }.trim().take(Protocol.MAX_FILE_NAME_CHARS)
        return last.takeIf { it.isNotEmpty() && it != "." && it != ".." }
    }

    /** A part of a diff, or null if it breaks the limits the computer keeps (so it cannot be a real one). */
    private fun diff(o: JSONObject): ServerMsg.Diff? {
        val parts = o.getInt("parts")
        val part = o.getInt("part")
        if (parts !in 1..Protocol.MAX_DIFF_PARTS || part !in 0 until parts) return null
        val arr = o.getJSONArray("lines")
        if (arr.length() > Protocol.PART_LINES) return null
        val rows = (0 until arr.length()).map { i ->
            val r = arr.getJSONArray(i)
            val k = r.getString(0)
            val kind = if (k.length == 1 && k[0] in "+-@") k[0] else ' '
            DiffRow(kind, r.getString(1).take(Protocol.MAX_DIFF_LINE_CHARS))
        }
        return ServerMsg.Diff(
            pillId = o.getString("pillId"), fileId = o.getLong("fileId"), name = fileNameOf(o.optString("name", "")).orEmpty(),
            added = o.optInt("added", 0).coerceAtLeast(0), removed = o.optInt("removed", 0).coerceAtLeast(0),
            tooLarge = o.optBoolean("tooLarge", false), gone = o.optBoolean("gone", false), truncated = o.optBoolean("truncated", false),
            part = part, parts = parts, lines = rows,
        )
    }

    /**
     * A question as the computer offers it, or null if it breaks the limits (the computer would not send such a
     * one): 1 to 4 questions, each with 1 to 8 options that have different, non-blank labels.
     */
    private fun question(o: JSONObject): QuestionRequest? {
        val fingerprint = o.getString("fingerprint").also { require(isFingerprint(it)) }
        val items = o.getJSONArray("questions").objects().map { q ->
            val options = q.getJSONArray("options").objects().map { AskedOption(it.getString("label"), it.optString("description", "")) }
            AskedQuestion(q.getString("question"), options, q.optBoolean("multiSelect", false))
        }
        val ok = items.isNotEmpty() && items.size <= Protocol.MAX_QUESTIONS && items.all { i ->
            i.question.isNotBlank() && i.options.isNotEmpty() && i.options.size <= Protocol.MAX_OPTIONS &&
                i.options.all { it.label.isNotBlank() } && i.options.map { it.label }.distinct().size == i.options.size
        }
        return if (ok) QuestionRequest(o.getString("pillId"), fingerprint, items, o.optLong("createdAt", 0)) else null
    }

    /** Strings of a JSON array, in order; anything else in it is skipped. */
    private fun stringList(a: JSONArray?): List<String> =
        if (a == null) emptyList() else (0 until a.length()).mapNotNull { (a.opt(it) as? String)?.takeIf { s -> s.isNotBlank() } }

    /** Even from a computer that should not send one, never a path: only the last segment is kept. */
    fun folderName(raw: String): String? {
        val last = raw.trimEnd('/', '\\').substringAfterLast('/').substringAfterLast('\\')
            .filter { !it.isISOControl() }.trim().take(Protocol.MAX_PROJECT_CHARS)
        return last.takeIf { it.isNotEmpty() && it != "." && it != ".." }
    }

    fun isColor(s: String) = s.length == 7 && s[0] == '#' && s.drop(1).all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }

    private fun strings(a: JSONArray?): Set<String> =
        if (a == null) emptySet() else (0 until a.length()).mapNotNull { a.optString(it, "").takeIf { s -> s.isNotEmpty() } }.toSet()

    /** A model without a usable id is dropped; a missing label shows the id. */
    private fun chatModel(o: JSONObject): ChatModel? {
        val id = o.optString("id", "")
        if (id.isBlank() || id.length > 300 || id.indexOf('/') <= 0) return null
        return ChatModel(id, o.optString("provider", id.substringBefore('/')), o.optString("label", "").ifBlank { id })
    }

    private fun chatId(o: JSONObject): String = o.getString("id").also { require(isChatId(it)) }

    private fun JSONArray.objects(): List<JSONObject> = (0 until length()).map { getJSONObject(it) }

    /** The id the phone makes for a chat message and the computer echoes: 1 to 64 of letters, digits, - and _. */
    fun isChatId(s: String) = s.isNotEmpty() && s.length <= 64 && s.all { it.isLetterOrDigit() && it.code < 128 || it == '-' || it == '_' }

    fun isFingerprint(s: String) = s.length == 64 && s.all { it in '0'..'9' || it in 'a'..'f' }
}

/** Same derivation as ApprovalRelay.fingerprint on the Mac: SHA-256 of the fields joined by U+001F. */
object Fingerprint {
    fun of(pillId: String, sessionId: String, tool: String, command: String, inputKey: String): String {
        val raw = listOf(pillId, sessionId, tool, command, inputKey).joinToString("\u001F")
        return MessageDigest.getInstance("SHA-256").digest(raw.toByteArray(Charsets.UTF_8))
            .joinToString("") { "%02x".format(it) }
    }
}
