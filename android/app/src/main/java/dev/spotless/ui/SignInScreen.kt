package dev.spotless.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import dev.spotless.Core
import dev.spotless.core.Action
import dev.spotless.core.Session

@Composable
fun SignInScreen(core: Core) {
    val session = core.session
    val status = when (session) {
        Session.SigningIn -> "WAITING FOR YOUR BROWSER"
        is Session.Failed -> "COULDN'T SIGN IN"
        else -> "NOT SIGNED IN"
    }
    Column(
        Modifier.fillMaxSize().background(Classic.frame).safeDrawingPadding().padding(horizontal = 24.dp),
        verticalArrangement = Arrangement.spacedBy(28.dp, Alignment.CenterVertically),
    ) {
        Column(
            Modifier.fillMaxWidth().lcd().padding(vertical = 28.dp, horizontal = 16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            BasicText(
                "SPOTLESS",
                style = body(20.sp, Classic.lcd, FontWeight.Bold).copy(letterSpacing = 0.4.em),
            )
            BasicText(status, style = mono(14.sp, Classic.dim))
        }
        if (session is Session.Failed) {
            BasicText(
                session.message,
                style = body(14.sp, Classic.label, FontWeight.Normal).copy(textAlign = TextAlign.Center),
            )
        }
        BasicText(
            "Spotless plays through your Spotify Premium account. Your browser opens once " +
                "to approve it; come back here when it says you're signed in.",
            style = body(15.sp, Classic.chrome, FontWeight.Normal).copy(lineHeight = 23.sp),
        )
        TextButton(
            if (session is Session.Failed) "SIGN IN AGAIN" else "SIGN IN WITH SPOTIFY",
            { core.apply(Action.SignIn) },
            Modifier.fillMaxWidth().height(56.dp),
            enabled = session != Session.SigningIn,
        )
    }
}
