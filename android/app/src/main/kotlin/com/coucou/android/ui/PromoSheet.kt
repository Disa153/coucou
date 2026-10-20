package com.coucou.android.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import com.coucou.android.R
import com.coucou.android.core.Promo
import com.coucou.android.core.Spacing
import com.coucou.android.core.TypeScale

/** Custom UI by sergey1029, once after an update (the computer shows the same card in the island). */
@Composable
fun PromoSheet(onDone: (never: Boolean) -> Unit) {
    val uri = LocalUriHandler.current
    var more by rememberSaveable { mutableStateOf(false) }
    var big by rememberSaveable { mutableStateOf(false) }
    val dim = Color.White.copy(alpha = 0.62f)
    BottomSheetHost(onDismiss = { onDone(false) }) {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            Image(painterResource(R.drawable.promo_customui), null, Modifier.size(44.dp).clip(RoundedCornerShape(10.dp)))
            Column {
                Text(stringResource(R.string.promo_tag), style = TypeScale.SECONDARY.style(dim))
                Text(stringResource(R.string.promo_title), style = TypeScale.HEADLINE.style(Color.White))
            }
        }
        // What it looks like; a tap shows it bigger (the words make room).
        Image(
            painterResource(R.drawable.promo_customui_shot), stringResource(R.string.promo_shot),
            Modifier.padding(top = 14.dp, bottom = 10.dp).fillMaxWidth().clip(RoundedCornerShape(12.dp))
                .border(1.dp, Color.White.copy(alpha = 0.12f), RoundedCornerShape(12.dp)).clickable(role = Role.Image) { big = !big },
            contentScale = if (big) ContentScale.FillWidth else ContentScale.Crop,
        )
        if (!big) {
            if (more) {
                listOf(R.string.promo_more_1, R.string.promo_more_2, R.string.promo_more_3, R.string.promo_more_4, R.string.promo_more_5, R.string.promo_more_6)
                    .forEach { Text("•  " + stringResource(it), style = TypeScale.SECONDARY.style(dim)) }
            } else {
                Text(stringResource(R.string.promo_line), style = TypeScale.BODY.style(dim))
            }
        }
        Row(Modifier.padding(top = 14.dp).fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (!more) PillButton(stringResource(R.string.promo_more), { more = true }, Modifier.weight(1f), onDark = true)
            PillButton(stringResource(R.string.promo_later), { onDone(false) }, Modifier.weight(1f), onDark = true)
            PillButton(stringResource(R.string.promo_open), { runCatching { uri.openUri(Promo.URL) }; onDone(false) }, Modifier.weight(1f), PillKind.PRIMARY, onDark = true)
        }
        Text(
            stringResource(R.string.promo_never),
            Modifier.align(Alignment.CenterHorizontally).heightIn(min = Spacing.MIN_TOUCH.dp).clickable(role = Role.Button) { onDone(true) }.padding(horizontal = 16.dp, vertical = 14.dp),
            style = TypeScale.SECONDARY.style(dim),
        )
    }
}
