package com.coucou.android.core

/**
 * A card after an update, the same as on the computer: once per new build, a friend's project (Custom UI,
 * a macOS-style shell for Windows 11 by sergey1029). Nothing is fetched: the words and pictures are in the
 * app, and the link opens only on the user's tap. The first start after installing shows nothing.
 */
data class PromoSeen(val lastBuild: Long = 0, val due: Boolean = false, val never: Boolean = false)

object Promo {
    const val URL = "https://github.com/sergey1029/custom-ui"

    /** What a start of [build] does to what was kept. */
    fun onStart(s: PromoSeen, build: Long): PromoSeen {
        if (build <= 0) return s
        val due = s.due || (s.lastBuild != 0L && build != s.lastBuild)
        return s.copy(lastBuild = build, due = due)
    }

    fun shown(s: PromoSeen): Boolean = s.due && !s.never

    fun done(s: PromoSeen, never: Boolean): PromoSeen = s.copy(due = false, never = s.never || never)
}
