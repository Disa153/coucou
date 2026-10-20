package com.coucou.android.ui

import android.content.ClipboardManager
import android.content.Context
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.coucou.android.R
import com.coucou.android.app.AppModel
import com.coucou.android.app.ChatChimes
import com.coucou.android.core.BetaChatText
import com.coucou.android.core.Spacing
import com.coucou.android.core.TypeScale
import com.coucou.android.link.BetaFile
import com.coucou.android.link.BetaMessage
import com.coucou.android.link.Protocol
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * The Chat tab: the AI chat as before and, when the computer offers it (its account is the owner's or a
 * tester's), the beta chat beside it.
 */
@Composable
fun ChatArea(model: AppModel, onHome: () -> Unit, onSettings: () -> Unit) {
    val beta = model.betaChat
    var showBeta by rememberSaveable { mutableStateOf(false) }
    if (!beta.offered) {
        ChatScreen(model, onHome, onSettings)
        return
    }
    Column(Modifier.fillMaxSize()) {
        Row(Modifier.padding(horizontal = Gutter, vertical = 6.dp).clip(CircleShape).background(tokens().panel2.c()).padding(3.dp)) {
            Segment(stringResource(R.string.beta_chat_tab_ai), !showBeta) { showBeta = false }
            val badge = if (beta.waiting > 0) " (${beta.waiting})" else ""
            Segment(stringResource(R.string.beta_chat_title) + badge, showBeta) { showBeta = true }
        }
        Box(Modifier.weight(1f)) {
            if (showBeta) BetaChatScreen(model) else ChatScreen(model, onHome, onSettings)
        }
    }
}

@Composable
private fun Segment(label: String, on: Boolean, onClick: () -> Unit) {
    val t = tokens()
    Text(
        label,
        Modifier.heightIn(min = 36.dp).clip(CircleShape).background(if (on) t.panel.c() else Color.Transparent)
            .clickable(role = Role.Tab, onClick = onClick).padding(horizontal = 16.dp, vertical = 9.dp),
        style = TypeScale.LABEL.style(if (on) t.text.c() else t.textDim.c()), maxLines = 1,
    )
}

/** A file picked to send: its name, type and bytes, and a small picture for a photo. */
private class Picked(val name: String, val mime: String, val bytes: ByteArray, val preview: android.graphics.Bitmap?)

private fun readPicked(context: Context, uri: Uri): Picked? = runCatching {
    val cr = context.contentResolver
    val mime = cr.getType(uri) ?: "application/octet-stream"
    var name = "file"
    cr.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
        if (c.moveToFirst()) c.getString(0)?.let { name = it }
    }
    if (mime.startsWith("image/") && mime != "image/gif") {
        // Photos go smaller, as JPEG, like the chat's photos (ChatPhotos).
        val photo = com.coucou.android.app.ChatPhotos.fromUri(context, uri) ?: return null
        return Picked(name.substringBeforeLast('.') + ".jpg", "image/jpeg", photo.jpeg, photo.preview)
    }
    val bytes = cr.openInputStream(uri)?.use { input ->
        val out = java.io.ByteArrayOutputStream()
        val buf = ByteArray(64 * 1024)
        while (true) {
            val n = input.read(buf)
            if (n < 0) break
            out.write(buf, 0, n)
            if (out.size() > Protocol.BETA_MAX_FILE_BYTES) return null
        }
        out.toByteArray()
    } ?: return null
    Picked(name, mime, bytes, null)
}.getOrNull()

/**
 * The beta chat: messages of the owner and the testers, the phone's own ones on the right with a grey
 * tick until the computer has sent them; photos inline; videos and files open in the phone's apps.
 * Read every 10 s while on screen.
 */
@Composable
fun BetaChatScreen(model: AppModel) {
    val beta = model.betaChat
    val t = tokens()
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var text by rememberSaveable { mutableStateOf("") }
    val picked = remember { mutableStateListOf<Picked>() }
    var note by remember { mutableStateOf<String?>(null) }
    var attachMenu by remember { mutableStateOf(false) }
    var soundMenu by remember { mutableStateOf(false) }
    val listState = rememberLazyListState()
    val uri = LocalUriHandler.current
    val tooBig = stringResource(R.string.beta_chat_too_big)
    val unreadable = stringResource(R.string.chat_photo_unreadable)
    val noClip = stringResource(R.string.beta_chat_no_clip_photo)
    val soundLabel = stringResource(R.string.beta_chat_sound)

    DisposableEffect(Unit) {
        beta.onScreen = true
        model.notifier.cancelBetaChat()
        onDispose { beta.onScreen = false }
    }
    LaunchedEffect(Unit) {
        beta.refresh(full = true)
        while (true) {
            delay(10_000)
            beta.refresh()
        }
    }
    val count = beta.messages.size + beta.pending.size
    LaunchedEffect(count) { if (count > 0) listState.animateScrollToItem(count - 1) }

    fun add(uris: List<Uri>) {
        scope.launch {
            for (u in uris) {
                if (picked.size >= Protocol.BETA_MAX_FILES) break
                val p = withContext(Dispatchers.IO) { readPicked(context, u) }
                if (p == null) note = unreadable
                else if (p.bytes.size > Protocol.BETA_MAX_FILE_BYTES) note = tooBig
                else picked.add(p)
            }
        }
    }
    val media = rememberLauncherForActivityResult(ActivityResultContracts.PickMultipleVisualMedia(Protocol.BETA_MAX_FILES)) { add(it) }
    val files = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { add(it) }
    fun pasteFromClipboard() {
        val clip = (context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).primaryClip
        val items = (0 until (clip?.itemCount ?: 0)).mapNotNull { clip?.getItemAt(it)?.uri }
        if (items.isEmpty()) note = noClip else add(items)
    }

    Column(Modifier.fillMaxSize().padding(horizontal = Gutter)) {
        ScreenTitle(stringResource(R.string.beta_chat_title), null, trailing = {
            Box {
                Text(
                    "🔔", Modifier.heightIn(min = Spacing.MIN_TOUCH.dp).clickable(role = Role.Button) { soundMenu = true }.padding(12.dp)
                        .semantics { contentDescription = soundLabel },
                    style = TypeScale.BODY.style(t.text.c()),
                )
                DropdownMenu(expanded = soundMenu, onDismissRequest = { soundMenu = false }) {
                    for (k in ChatChimes.KINDS) {
                        DropdownMenuItem(
                            text = { Text((if (k == beta.sound) "✓ " else "    ") + soundName(k)) },
                            onClick = { beta.pickSound(k) },
                        )
                    }
                    Column(Modifier.padding(horizontal = 16.dp).widthIn(min = 200.dp)) {
                        Text(stringResource(R.string.beta_chat_volume), style = TypeScale.SECONDARY.style(t.textDim.c()))
                        Slider(value = beta.volume, onValueChange = { beta.pickVolume(it) }, onValueChangeFinished = { ChatChimes.play(beta.sound, beta.volume) })
                    }
                }
            }
            if (beta.page.isNotEmpty()) {
                Text(
                    stringResource(R.string.beta_chat_on_github),
                    Modifier.heightIn(min = Spacing.MIN_TOUCH.dp).clickable(role = Role.Button) { runCatching { uri.openUri(beta.page) } }.padding(12.dp),
                    style = TypeScale.LABEL.style(t.textDim.c()), maxLines = 1,
                )
            }
        })
        Text(stringResource(R.string.beta_chat_public), Modifier.padding(horizontal = 4.dp, vertical = 2.dp), style = TypeScale.SECONDARY.style(t.textDim.c()))
        beta.error?.let { Text(betaError(it), Modifier.padding(4.dp), style = TypeScale.SECONDARY.style(t.danger.c())) }
        Box(Modifier.weight(1f).fillMaxWidth()) {
            if (count == 0) {
                Text(
                    stringResource(R.string.beta_chat_empty), Modifier.align(Alignment.Center).padding(Gutter),
                    style = TypeScale.BODY.style(t.textDim.c()).copy(textAlign = TextAlign.Center),
                )
            } else {
                LazyColumn(state = listState, modifier = Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    items(beta.messages, key = { it.id }) { m -> BetaBubble(model, m) }
                    items(beta.pending, key = { it.key }) { p ->
                        Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.End) {
                            Text(
                                (p.fileNames.joinToString("\n") { "📎 $it" } + "\n" + p.text).trim(),
                                Modifier.widthIn(max = 300.dp).clip(RoundedCornerShape(16.dp, 16.dp, 4.dp, 16.dp))
                                    .background(Color(0xFF3D5AFE).copy(alpha = 0.6f)).padding(horizontal = 12.dp, vertical = 8.dp),
                                style = TypeScale.BODY.style(Color.White),
                            )
                            if (p.failed) {
                                Text(
                                    stringResource(R.string.beta_chat_retry),
                                    Modifier.clickable(role = Role.Button) { beta.retry(p.key) }.padding(6.dp),
                                    style = TypeScale.SECONDARY.style(t.danger.c()),
                                )
                            } else {
                                Text(stringResource(R.string.beta_chat_sending) + " ✓", style = TypeScale.SECONDARY.style(t.textDim.c()))
                            }
                        }
                    }
                }
            }
        }
        if (picked.isNotEmpty()) {
            Row(Modifier.horizontalScroll(rememberScrollState()).padding(vertical = 4.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                picked.forEachIndexed { i, p ->
                    Box {
                        if (p.preview != null) {
                            Image(p.preview.asImageBitmap(), null, Modifier.size(56.dp).clip(RoundedCornerShape(10.dp)), contentScale = ContentScale.Crop)
                        } else {
                            Box(Modifier.size(56.dp).clip(RoundedCornerShape(10.dp)).background(t.panel2.c()), contentAlignment = Alignment.Center) {
                                Text(if (p.mime.startsWith("video/")) "🎬" else "📄", style = TypeScale.TITLE.style(t.text.c()))
                            }
                        }
                        val remove = stringResource(R.string.chat_photo_remove)
                        Box(
                            Modifier.align(Alignment.TopEnd).padding(2.dp).size(20.dp).clip(CircleShape).background(t.panel.c())
                                .clickable(role = Role.Button) { if (i < picked.size) picked.removeAt(i) }.semantics { contentDescription = remove },
                            contentAlignment = Alignment.Center,
                        ) { Text("×", style = TypeScale.LABEL.style(t.text.c())) }
                    }
                }
            }
        }
        Row(Modifier.padding(vertical = 8.dp), verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Box {
                val attach = stringResource(R.string.beta_chat_attach)
                Box(
                    Modifier.size(44.dp).clip(CircleShape).background(t.panel2.c()).clickable(role = Role.Button) { attachMenu = true }
                        .semantics { contentDescription = attach },
                    contentAlignment = Alignment.Center,
                ) { Text("📎", style = TypeScale.BODY.style(t.text.c())) }
                DropdownMenu(expanded = attachMenu, onDismissRequest = { attachMenu = false }) {
                    DropdownMenuItem(text = { Text(stringResource(R.string.beta_chat_pick_media)) }, onClick = {
                        attachMenu = false
                        media.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageAndVideo))
                    })
                    DropdownMenuItem(text = { Text(stringResource(R.string.beta_chat_pick_file)) }, onClick = {
                        attachMenu = false
                        files.launch(arrayOf("*/*"))
                    })
                    DropdownMenuItem(text = { Text(stringResource(R.string.beta_chat_paste_photo)) }, onClick = {
                        attachMenu = false
                        pasteFromClipboard()
                    })
                }
            }
            OutlinedTextField(
                value = text, onValueChange = { text = it.take(Protocol.BETA_MAX_TEXT) }, modifier = Modifier.weight(1f),
                placeholder = { Text(stringResource(R.string.beta_chat_placeholder), color = t.textFaint.c()) },
                maxLines = 4, shape = RoundedCornerShape(24.dp),
                colors = OutlinedTextFieldDefaults.colors(
                    focusedContainerColor = t.panel.c(), unfocusedContainerColor = t.panel.c(),
                    focusedBorderColor = t.textDim.c(), unfocusedBorderColor = t.line.c(),
                    focusedTextColor = t.text.c(), unfocusedTextColor = t.text.c(), cursorColor = t.text.c(),
                ),
            )
            val ready = text.isNotBlank() || picked.isNotEmpty()
            val send = stringResource(R.string.chat_send)
            Box(
                Modifier.alpha(if (ready) 1f else 0.4f).size(52.dp).clip(CircleShape).background(t.primaryButton.c())
                    .clickable(enabled = ready, role = Role.Button) {
                        if (beta.send(text, picked.map { Triple(it.name, it.mime, it.bytes) })) {
                            text = ""
                            picked.clear()
                            note = null
                        }
                    }.semantics { contentDescription = send },
                contentAlignment = Alignment.Center,
            ) { CoucouIcon(com.coucou.android.core.IconKind.SEND, tint = t.onPrimaryButton.c(), size = 22.dp) }
        }
        note?.let { Text(it, Modifier.padding(bottom = 6.dp), style = TypeScale.SECONDARY.style(t.textDim.c())) }
    }
}

@Composable
private fun soundName(kind: String): String = stringResource(
    when (kind) {
        "hee" -> R.string.beta_sound_hee
        "quack" -> R.string.beta_sound_quack
        "knock" -> R.string.beta_sound_knock
        "mochi" -> R.string.beta_sound_mochi
        "none" -> R.string.beta_sound_none
        else -> R.string.beta_sound_chime
    },
)

@Composable
private fun betaError(code: String): String = stringResource(
    when (code) {
        "notSignedIn", "notAllowed" -> R.string.beta_chat_err_account
        "signInAgain" -> R.string.beta_chat_err_sign_in
        "tooBig" -> R.string.beta_chat_too_big
        else -> R.string.beta_chat_err_network
    },
)

@Composable
private fun BetaBubble(model: AppModel, m: BetaMessage) {
    val beta = model.betaChat
    val t = tokens()
    var menu by remember { mutableStateOf(false) }
    Row(Modifier.fillMaxWidth(), horizontalArrangement = if (m.mine) Arrangement.End else Arrangement.Start) {
        if (!m.mine) {
            Box(Modifier.padding(top = 18.dp, end = 6.dp).size(26.dp).clip(CircleShape).background(Color(BetaChatText.color(m.login))), contentAlignment = Alignment.Center) {
                Text(m.login.take(1).uppercase(), style = TypeScale.LABEL.style(Color(0xFF0B0C0E)))
            }
        }
        Column(horizontalAlignment = if (m.mine) Alignment.End else Alignment.Start) {
            val who = "@" + m.login + if (m.owner) " · " + stringResource(R.string.beta_chat_owner) else ""
            Text(who, style = TypeScale.SECONDARY.style(if (m.owner) Color(0xFFF5A524) else t.textDim.c()), maxLines = 1, overflow = TextOverflow.Ellipsis)
            Column(
                Modifier.widthIn(max = 290.dp)
                    .clip(if (m.mine) RoundedCornerShape(16.dp, 16.dp, 4.dp, 16.dp) else RoundedCornerShape(16.dp, 16.dp, 16.dp, 4.dp))
                    .background(if (m.mine) Color(0xFF3D5AFE) else t.panel2.c())
                    .clickable(enabled = m.mine || beta.owner) { menu = true }
                    .padding(horizontal = 12.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                m.build?.let { Text(stringResource(R.string.beta_chat_build, it), style = TypeScale.SECONDARY.style(Color(0xFFF5A524))) }
                if (m.text.isNotEmpty()) SelectionContainer { Text(m.text, style = TypeScale.BODY.style(if (m.mine) Color.White else t.text.c())) }
                for (f in m.files) BetaAttachment(model, f, m.mine)
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    DropdownMenuItem(text = { Text(stringResource(R.string.beta_chat_delete)) }, onClick = { menu = false; beta.delete(m.id) })
                }
            }
            if (m.atSec > 0) {
                val time = remember(m.atSec) {
                    java.text.DateFormat.getTimeInstance(java.text.DateFormat.SHORT).format(java.util.Date(m.atSec * 1000))
                }
                Text(if (m.mine) "$time ✓✓" else time, style = TypeScale.SECONDARY.style(t.textDim.c()))
            }
        }
    }
}

@Composable
private fun BetaAttachment(model: AppModel, f: BetaFile, mine: Boolean) {
    val beta = model.betaChat
    val t = tokens()
    val ink = if (mine) Color.White else t.text.c()
    if (f.mime.startsWith("image/")) {
        val bmp = beta.images[f.url]
        LaunchedEffect(f.url) { beta.fetch(f) }
        if (bmp != null) {
            Image(bmp.asImageBitmap(), f.name, Modifier.widthIn(max = 240.dp).heightIn(max = 240.dp).clip(RoundedCornerShape(10.dp)), contentScale = ContentScale.Fit)
        } else {
            Text("🖼 ${f.name}", style = TypeScale.SECONDARY.style(ink))
        }
        return
    }
    val busy = beta.loading[f.url] == true
    val icon = if (f.mime.startsWith("video/")) "▶" else "📄"
    Text(
        if (busy) stringResource(R.string.beta_chat_loading) else "$icon ${f.name} · ${BetaChatText.size(f.size)}",
        Modifier.clip(RoundedCornerShape(8.dp)).background(Color.Black.copy(alpha = 0.2f)).clickable(enabled = !busy, role = Role.Button) { beta.fetch(f) }
            .padding(horizontal = 8.dp, vertical = 6.dp),
        style = TypeScale.SECONDARY.style(ink),
    )
}
