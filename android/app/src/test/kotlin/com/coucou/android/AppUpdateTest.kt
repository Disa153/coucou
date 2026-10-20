package com.coucou.android

import com.coucou.android.core.AppUpdate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AppUpdateTest {
    private val sha = "a".repeat(64)
    private fun manifest(code: Long = 172, url: String = "https://github.com/Disa153/coucou/releases/download/all-in-one-latest/Coucou-Android.apk", hash: String = sha) =
        """{"versionCode":$code,"versionName":"0.2.0-fork.72","url":"$url","sha256":"$hash","size":12345678}"""

    @Test fun readsTheManifest() {
        val r = AppUpdate.parse(manifest())
        assertNotNull(r)
        assertEquals(172L, r!!.versionCode)
        assertEquals("0.2.0-fork.72", r.versionName)
        assertEquals(12345678L, r.size)
    }

    @Test fun refusesAFileFromElsewhere() {
        assertNull(AppUpdate.parse(manifest(url = "https://example.com/Coucou.apk")))
        assertNull(AppUpdate.parse(manifest(url = "https://github.com/Disa153/coucou/releases/download/../../evil/x.apk")))
        assertNull(AppUpdate.parse(manifest(url = "http://github.com/Disa153/coucou/releases/download/a/b.apk")))
    }

    @Test fun refusesABadHash() {
        assertNull(AppUpdate.parse(manifest(hash = "abc")))
        assertNull(AppUpdate.parse(manifest(hash = "z".repeat(64))))
        assertNotNull(AppUpdate.parse(manifest(hash = "A".repeat(64))))
        assertNull(AppUpdate.parse("not json"))
        assertNull(AppUpdate.parse(manifest(code = 0)))
    }

    @Test fun offersOnlyANewerBuild() {
        val r = AppUpdate.parse(manifest(code = 172))
        assertNotNull(AppUpdate.newer(r, 171))
        assertNull(AppUpdate.newer(r, 172))
        assertNull(AppUpdate.newer(r, 180))
        assertNull(AppUpdate.newer(null, 1))
    }

    @Test fun asksEverySixHours() {
        val h = 60L * 60 * 1000
        assertTrue(AppUpdate.due(0, 7 * h))
        assertFalse(AppUpdate.due(10 * h, 11 * h))
        assertTrue(AppUpdate.due(10 * h, 16 * h))
        assertTrue(AppUpdate.due(10 * h, 5 * h))
    }

    @Test fun hexIsLowerCase() = assertEquals("00ff10", AppUpdate.hex(byteArrayOf(0, -1, 16)))
}


class PromoTest {
    @Test fun notOnTheFirstStartOnlyAfterAnUpdate() {
        var s = com.coucou.android.core.Promo.onStart(com.coucou.android.core.PromoSeen(), 174)
        assertFalse(com.coucou.android.core.Promo.shown(s))
        s = com.coucou.android.core.Promo.onStart(s, 174)
        assertFalse(com.coucou.android.core.Promo.shown(s))
        s = com.coucou.android.core.Promo.onStart(s, 175)
        assertTrue(com.coucou.android.core.Promo.shown(s))
        s = com.coucou.android.core.Promo.onStart(s, 175) // still due until shown
        assertTrue(com.coucou.android.core.Promo.shown(s))
    }

    @Test fun laterWaitsForTheNextUpdateNeverEndsIt() {
        val due = com.coucou.android.core.PromoSeen(175, due = true)
        val later = com.coucou.android.core.Promo.done(due, never = false)
        assertFalse(com.coucou.android.core.Promo.shown(later))
        assertTrue(com.coucou.android.core.Promo.shown(com.coucou.android.core.Promo.onStart(later, 176)))
        val never = com.coucou.android.core.Promo.done(due, never = true)
        assertFalse(com.coucou.android.core.Promo.shown(com.coucou.android.core.Promo.onStart(never, 176)))
    }
}
