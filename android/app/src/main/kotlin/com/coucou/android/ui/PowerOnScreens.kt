package com.coucou.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import com.coucou.android.R
import com.coucou.android.app.AppModel
import com.coucou.android.core.Spacing
import com.coucou.android.core.TypeScale
import com.coucou.android.link.LinkState
import com.coucou.android.link.PowerOn
import com.coucou.android.link.RouterLogin
import com.coucou.android.link.RouterProblem

/** What happened after "Turn on computer", in words; null when there is nothing to say. */
@Composable
private fun powerText(model: AppModel): String? = when (model.powerPhase) {
    AppModel.PowerPhase.IDLE -> null
    AppModel.PowerPhase.SENDING -> stringResource(R.string.power_sending)
    AppModel.PowerPhase.WAITING -> stringResource(if (model.powerByRouter) R.string.power_waiting_router else R.string.power_waiting_phone)
    AppModel.PowerPhase.ON -> stringResource(R.string.power_on)
    AppModel.PowerPhase.FAILED -> when {
        model.wakeTarget == null -> stringResource(R.string.power_no_card)
        model.powerProblem != null -> stringResource(R.string.power_router_failed, routerProblemText(model.powerProblem))
        else -> stringResource(R.string.power_timeout)
    }
}

@Composable
private fun routerProblemText(p: RouterProblem?): String = stringResource(
    when (p) {
        RouterProblem.NOT_KEENETIC -> R.string.router_not_keenetic
        RouterProblem.BAD_LOGIN -> R.string.router_bad_login
        RouterProblem.REFUSED -> R.string.router_refused
        else -> R.string.router_unreachable
    },
)

/** The button and what happened, on Home (in the "connection lost" panel) and in Settings. */
@Composable
fun PowerOnRow(model: AppModel) {
    val t = tokens()
    Column(Modifier.padding(Spacing.INSIDE.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        PillButton(
            stringResource(if (model.powerPhase == AppModel.PowerPhase.SENDING) R.string.power_sending else R.string.power_button),
            { model.powerOn() }, Modifier.fillMaxWidth(), PillKind.PRIMARY,
            enabled = model.powerPhase != AppModel.PowerPhase.SENDING && model.linkState != LinkState.CONNECTED,
        )
        powerText(model)?.let {
            val bad = model.powerPhase == AppModel.PowerPhase.FAILED
            Text(it, style = TypeScale.SECONDARY.style(if (bad) t.danger.c() else t.textDim.c()))
        }
    }
}

@Composable
private fun Field(value: String, onChange: (String) -> Unit, label: String, password: Boolean = false, keyboard: KeyboardType = KeyboardType.Text) {
    val t = tokens()
    OutlinedTextField(
        value = value, onValueChange = onChange, modifier = Modifier.fillMaxWidth(), singleLine = true,
        label = { Text(label) },
        visualTransformation = if (password) PasswordVisualTransformation() else androidx.compose.ui.text.input.VisualTransformation.None,
        keyboardOptions = KeyboardOptions(keyboardType = if (password) KeyboardType.Password else keyboard, autoCorrectEnabled = false),
        shape = RoundedCornerShape(14.dp),
        colors = OutlinedTextFieldDefaults.colors(
            focusedContainerColor = t.panel.c(), unfocusedContainerColor = t.panel.c(),
            focusedBorderColor = t.textDim.c(), unfocusedBorderColor = t.line.c(),
            focusedTextColor = t.text.c(), unfocusedTextColor = t.text.c(), cursorColor = t.text.c(),
        ),
    )
}

/** Settings → Turn on computer: the card the computer told us, the button, the Keenetic router, a card typed by hand. */
@Composable
fun PowerOnCard(model: AppModel) {
    val t = tokens()
    val r = model.router
    var address by remember { mutableStateOf(r.address) }
    var away by remember { mutableStateOf(r.awayAddress) }
    var login by remember { mutableStateOf(r.login) }
    var password by remember { mutableStateOf(r.password) }
    var mac by remember { mutableStateOf("") }
    var badMac by remember { mutableStateOf(false) }
    Panel {
        Row(Modifier.padding(Spacing.INSIDE.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(stringResource(R.string.power_card), Modifier.weight(1f), style = TypeScale.BODY.style(t.text.c()))
            Text(
                model.wakeTarget?.mac?.let { PowerOn.colonMac(it) } ?: stringResource(R.string.power_card_unknown),
                style = TypeScale.SECONDARY.style(t.textDim.c()),
            )
        }
        RowDivider()
        PowerOnRow(model)
        Text(
            stringResource(R.string.power_hint), Modifier.padding(start = Spacing.INSIDE.dp, end = Spacing.INSIDE.dp, bottom = Spacing.INSIDE.dp),
            style = TypeScale.SECONDARY.style(t.textDim.c()),
        )
        RowDivider()
        Column(Modifier.padding(Spacing.INSIDE.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(stringResource(R.string.router_title), style = TypeScale.HEADLINE.style(t.text.c()))
            Field(address, { address = it }, stringResource(R.string.router_address), keyboard = KeyboardType.Uri)
            Field(away, { away = it }, stringResource(R.string.router_away), keyboard = KeyboardType.Uri)
            Field(login, { login = it }, stringResource(R.string.router_login))
            Field(password, { password = it }, stringResource(R.string.router_password), password = true)
            val draft = RouterLogin(address.trim(), away.trim(), login.trim(), password)
            PillButton(
                stringResource(if (model.routerChecking) R.string.router_checking else R.string.router_check),
                { model.checkRouter(draft) }, Modifier.fillMaxWidth(), enabled = !model.routerChecking && draft.complete,
            )
            model.routerCheck?.let { result ->
                val text = if (result.startsWith("ok:")) stringResource(R.string.router_ok, result.removePrefix("ok:"))
                else routerProblemText(runCatching { RouterProblem.valueOf(result.removePrefix("err:")) }.getOrNull())
                Text(text, style = TypeScale.SECONDARY.style(if (result.startsWith("ok:")) t.textDim.c() else t.danger.c()))
            }
            Text(stringResource(R.string.router_hint), style = TypeScale.SECONDARY.style(t.textDim.c()))
        }
        RowDivider()
        Column(Modifier.padding(Spacing.INSIDE.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Field(mac, { mac = it; badMac = false }, stringResource(R.string.power_mac_by_hand))
            PillButton(stringResource(R.string.power_mac_use), {
                badMac = !model.setWakeMac(mac)
                if (!badMac) mac = ""
            }, Modifier.fillMaxWidth(), enabled = mac.isNotBlank())
            Text(
                stringResource(if (badMac) R.string.power_mac_bad else R.string.power_mac_hint),
                style = TypeScale.SECONDARY.style(if (badMac) t.danger.c() else t.textDim.c()),
            )
        }
    }
}
