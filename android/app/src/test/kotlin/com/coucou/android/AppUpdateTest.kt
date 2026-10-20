
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
