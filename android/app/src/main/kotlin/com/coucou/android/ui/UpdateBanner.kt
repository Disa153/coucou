package com.coucou.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import com.coucou.android.R
import com.coucou.android.app.AppUpdater
import com.coucou.android.core.BetaChatText
import com.coucou.android.core.Spacing
import com.coucou.android.core.TypeScale

/** "Update available" at the top of the screen, until it is installed or put away for this build. */
@Composable
fun UpdateBanner(updater: AppUpdater, modifier: Modifier = Modifier) {
    val r = updater.available ?: return
    if (!updater.showBanner) return
    val t = tokens()
    Column(
        modifier.fillMaxWidth().padding(horizontal = Gutter, vertical = 8.dp).clip(RoundedCornerShape(18.dp)).background(t.panel2.c())
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(stringResource(R.string.update_title, r.versionName.ifBlank { r.versionCode.toString() }), style = TypeScale.HEADLINE.style(t.text.c()))
        val line = when (updater.phase) {
            AppUpdater.Phase.DOWNLOADING -> stringResource(R.string.update_downloading)
            AppUpdater.Phase.VERIFYING -> stringResource(R.string.update_verifying)
            AppUpdater.Phase.FAILED -> stringResource(R.string.update_failed)
            AppUpdater.Phase.READY -> stringResource(R.string.update_ready)
            AppUpdater.Phase.IDLE -> stringResource(R.string.update_body, if (r.size > 0) BetaChatText.size(r.size) else "")
        }
        Text(line, style = TypeScale.SECONDARY.style(if (updater.phase == AppUpdater.Phase.FAILED) t.danger.c() else t.textDim.c()))
        if (updater.phase == AppUpdater.Phase.DOWNLOADING) LinearProgressIndicator(progress = { updater.progress }, modifier = Modifier.fillMaxWidth())
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(
                stringResource(R.string.update_later),
                Modifier.heightIn(min = Spacing.MIN_TOUCH.dp).clickable(role = Role.Button) { updater.dismiss() }.padding(horizontal = 8.dp, vertical = 12.dp),
                style = TypeScale.LABEL.style(t.textDim.c()),
            )
            Column(Modifier.weight(1f)) {}
            val busy = updater.phase == AppUpdater.Phase.DOWNLOADING || updater.phase == AppUpdater.Phase.VERIFYING
            PillButton(
                stringResource(if (updater.phase == AppUpdater.Phase.READY) R.string.update_open_installer else R.string.update_install),
                { if (updater.phase == AppUpdater.Phase.READY) updater.openInstaller() else updater.install() },
                kind = PillKind.PRIMARY, enabled = !busy,
            )
        }
    }
}
