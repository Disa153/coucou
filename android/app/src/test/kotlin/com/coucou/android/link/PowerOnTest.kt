package com.coucou.android.link

import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** "Turn on computer" (Wake-on-LAN and the Keenetic router): the pure parts. */
class PowerOnTest {
    @Test fun macsAreReadOneWay() {
        assertArrayEquals(byteArrayOf(0xd8.toByte(), 0xbb.toByte(), 0xc1.toByte(), 0x11, 0x22, 0x33), PowerOn.macBytes("D8-BB-C1-11-22-33"))
        assertEquals("d8:bb:c1:11:22:33", PowerOn.colonMac("d8:bb:c1:11:22:33"))
        assertNull(PowerOn.macBytes("00:00:00:00:00:00"))
        assertNull(PowerOn.macBytes("d8:bb:c1"))
        assertNull(PowerOn.macBytes("zz:bb:c1:11:22:33"))
    }

    @Test fun theMagicPacketIsSixFfThenTheMacSixteenTimes() {
        val mac = PowerOn.macBytes("01:02:03:04:05:06")!!
        val p = PowerOn.packet(mac)
        assertEquals(102, p.size)
        assertTrue(p.take(6).all { it == 0xFF.toByte() })
        for (i in 0 until 16) assertArrayEquals(mac, p.copyOfRange(6 + i * 6, 12 + i * 6))
    }

    @Test fun keeneticSignsInWithItsChallenge() {
        assertEquals(
            "7dda227ce9eb1c26979d567af2ef791bbe771edb28c7cf152b5116a81d3769dd",
            PowerOn.keeneticPassword("admin", "Keenetic Ultra", "secret", "CHALLENGE123"),
        )
    }

    @Test fun homeAddressesAreHttpAndKeenDnsHttps() {
        assertEquals("http://192.168.1.1", PowerOn.base("192.168.1.1/"))
        assertEquals("https://disa153.keenetic.link", PowerOn.base("disa153.keenetic.link"))
        assertEquals("http://router.local", PowerOn.base("router.local"))
        assertEquals("https://x.example", PowerOn.base("https://x.example"))
    }

    @Test fun theRouterTellsTheWiredCardByTheComputersName() {
        val json = JSONObject().put(
            "host",
            org.json.JSONArray()
                .put(JSONObject().put("hostname", "DESKTOP-1").put("mac", "aa:aa:aa:aa:aa:01").put("ssid", "Home").put("active", true))
                .put(JSONObject().put("hostname", "DESKTOP-1").put("mac", "aa:aa:aa:aa:aa:02").put("active", false))
                .put(JSONObject().put("name", "Phone").put("mac", "aa:aa:aa:aa:aa:03")),
        ).toString()
        assertEquals("aa:aa:aa:aa:aa:02", PowerOn.pickMac(json, "desktop-1"))
        assertNull(PowerOn.pickMac(json, "Laptop"))
        assertNull(PowerOn.pickMac("not json", "DESKTOP-1"))
    }

    @Test fun theRouterLoginTriesHomeFirst() {
        val l = RouterLogin(" 192.168.1.1 ", "me.keenetic.link", "coucou", "pw")
        assertEquals(listOf("192.168.1.1", "me.keenetic.link"), l.addresses)
        assertTrue(l.complete)
        assertTrue(!RouterLogin("", "", "coucou", "pw").complete)
    }

    @Test fun theWelcomeCarriesTheCardAndPhotosGoInParts() {
        val w = Wire.decodeServer("""{"type":"welcome","v":1,"desktop":"PC","os":"windows","caps":["chat"],"wake":{"mac":"d8:bb:c1:11:22:33","ip":"192.168.1.5","broadcast":"192.168.1.255"}}""")
        assertEquals(WakeTarget("d8:bb:c1:11:22:33", "192.168.1.5", "192.168.1.255"), (w as ServerMsg.Welcome).wake)
        val none = Wire.decodeServer("""{"type":"welcome","v":1,"desktop":"PC","os":"windows","wake":{"mac":"nope"}}""")
        assertNull((none as ServerMsg.Welcome).wake)
        val send = JSONObject(Wire.encode(ClientMsg.ChatSend("c1", "claude-code/default", "look", listOf("img-1"))))
        assertEquals("img-1", send.getJSONArray("images").getString(0))
        assertTrue(!JSONObject(Wire.encode(ClientMsg.ChatSend("c1", "m/x", "a"))).has("images"))
        val part = JSONObject(Wire.encode(ClientMsg.ChatImage("img-1", 0, 2, "AAAA")))
        assertEquals("chatImage", part.getString("type"))
        assertEquals(2, part.getInt("parts"))
    }
}
