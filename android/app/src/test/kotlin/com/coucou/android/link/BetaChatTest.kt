package com.coucou.android.link

import com.coucou.android.core.BetaChatText
import com.coucou.android.core.BetaDownAssembler
import com.coucou.android.core.BetaPageAssembler
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The beta chat through the computer (cap `betachat`): the lines both ways and the parts put back together. */
class BetaChatTest {
    private fun msg(id: Long, text: String = "hi", files: String = "[]") =
        """{"id":$id,"login":"sergey1029","text":"$text","build":65,"files":$files,"at":1792000000,"owner":false,"mine":false}"""

    @Test fun thePhoneAsksForTheBetaChat() {
        assertTrue(Protocol.CAP_BETA_CHAT in Protocol.CAPABILITIES)
        val o = JSONObject(Wire.encode(ClientMsg.BetaChatSend("k1", "hello", listOf(BetaOutFile("k1-0", "a.jpg", "image/jpeg")))))
        assertEquals("betaChatSend", o.getString("type"))
        assertEquals("k1-0", o.getJSONArray("files").getJSONObject(0).getString("fid"))
        assertEquals("betaChatGet", JSONObject(Wire.encode(ClientMsg.BetaChatGet(true))).getString("type"))
        assertEquals(42L, JSONObject(Wire.encode(ClientMsg.BetaChatDelete(42))).getLong("id"))
    }

    @Test fun aPageReadsBackAndFilesComeOnlyFromAGist() {
        val files = """[{"name":"s.jpg","mime":"image/jpeg","size":10,"url":"https://gist.githubusercontent.com/u/a/raw/b/s.jpg.b64"},
            {"name":"x","mime":"text/plain","size":1,"url":"https://example.com/x"}]"""
        val line = """{"type":"betaChatPage","part":0,"parts":1,"owner":true,"page":"https://github.com/Disa153/coucou/issues/30","messages":[${msg(7, files = files)}]}"""
        val p = Wire.decodeServer(line) as ServerMsg.BetaChatPage
        assertEquals(1, p.messages.size)
        val m = p.messages[0]
        assertEquals("sergey1029", m.login)
        assertEquals(65L, m.build)
        assertEquals(listOf("s.jpg"), m.files.map { it.name })
        assertTrue(p.owner)
        // a page link that is not GitHub's is dropped
        val odd = Wire.decodeServer(line.replace("https://github.com/Disa153/coucou/issues/30", "javascript:x")) as ServerMsg.BetaChatPage
        assertEquals("", odd.page)
    }

    @Test fun thePageComesTogetherFromItsParts() {
        val a = BetaPageAssembler()
        fun part(i: Int, n: Int, vararg ids: Long) = ServerMsg.BetaChatPage(i, n, false, "", ids.map { BetaMessage(it, "a", "t", null, emptyList(), 0, false, false) }, null)
        assertNull(a.add(part(0, 2, 1, 2)))
        assertEquals(listOf(1L, 2L, 3L), a.add(part(1, 2, 3))!!.map { it.id })
        // a fresh first part starts over
        assertNull(a.add(part(0, 3, 1)))
        assertNull(a.add(part(0, 2, 5)))
        assertEquals(listOf(5L, 6L), a.add(part(1, 2, 6))!!.map { it.id })
    }

    @Test fun aFileComesTogetherFromItsParts() {
        val bytes = ByteArray(100_000) { (it % 251).toByte() }
        val b64 = java.util.Base64.getEncoder().encodeToString(bytes)
        val parts = b64.chunked(40_000)
        val d = BetaDownAssembler()
        parts.dropLast(1).forEachIndexed { i, s -> assertNull(d.add(ServerMsg.BetaChatDown("u", i, parts.size, s, null))) }
        val whole = d.add(ServerMsg.BetaChatDown("u", parts.size - 1, parts.size, parts.last(), null))
        assertTrue(whole!!.contentEquals(bytes))
        // an error drops what came
        assertNull(d.add(ServerMsg.BetaChatDown("v", 0, 2, "QQ==", null)))
        assertNull(d.add(ServerMsg.BetaChatDown("v", 0, 1, "", "network")))
    }

    @Test fun newsAndSizesReadAsShown() {
        val n = Wire.decodeServer("""{"type":"betaChatNews","count":2,"login":"anna","text":"67 works"}""") as ServerMsg.BetaChatNews
        assertEquals(2, n.waiting)
        assertEquals("anna", n.login)
        assertEquals(ServerMsg.BetaChatChanged, Wire.decodeServer("""{"type":"betaChatChanged"}"""))
        assertEquals("1.5 MB", BetaChatText.size(1_572_864))
        assertEquals("2 KB", BetaChatText.size(2_048))
        // the same colour as on the computer for the same login
        assertEquals(BetaChatText.color("sergey1029"), BetaChatText.color("SERGEY1029"))
    }
}
