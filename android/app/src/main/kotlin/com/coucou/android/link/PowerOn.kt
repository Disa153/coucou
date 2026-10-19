package com.coucou.android.link

import java.io.IOException
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.HttpURLConnection
import java.net.InetAddress
import java.net.URL
import java.security.MessageDigest
import org.json.JSONArray
import org.json.JSONObject

/** The Keenetic router, to send the magic packet for us: [address] at home, [awayAddress] (KeenDNS) elsewhere. */
data class RouterLogin(val address: String, val awayAddress: String, val login: String, val password: String) {
    val complete: Boolean get() = (address.isNotBlank() || awayAddress.isNotBlank()) && login.isNotBlank() && password.isNotEmpty()
    /** The addresses to try, home first. */
    val addresses: List<String> get() = listOf(address, awayAddress).map { it.trim() }.filter { it.isNotEmpty() }
}

/** Why the router did not do it; the screen words it. */
enum class RouterProblem { UNREACHABLE, NOT_KEENETIC, BAD_LOGIN, REFUSED }

class RouterException(val problem: RouterProblem, val detail: String = "") : Exception(detail)

/**
 * "Turn on computer": Wake-on-LAN, as on the iPhone (NotchBuddy/Sources/Phone/LANPowerOn.swift). The magic packet goes
 * from this phone to the network's broadcast address, and, when one is set up, the Keenetic router sends it too
 * (`ip hotspot wake`), at home or through its KeenDNS name. Blocking: called off the main thread.
 */
object PowerOn {
    /** "aa:bb:cc:dd:ee:ff" (or with dashes) → its six bytes; null for anything else or all zeros. */
    fun macBytes(mac: String): ByteArray? {
        val hex = mac.filter { it != ':' && it != '-' && it != '.' && !it.isWhitespace() }
        if (hex.length != 12 || !hex.all { it.isDigit() || it.lowercaseChar() in 'a'..'f' }) return null
        val out = ByteArray(6) { i -> hex.substring(i * 2, i * 2 + 2).toInt(16).toByte() }
        return out.takeIf { b -> b.any { it.toInt() != 0 } }
    }

    private fun hex2(b: Byte): String {
        val v = b.toInt() and 0xff
        return "0123456789abcdef"[v shr 4].toString() + "0123456789abcdef"[v and 15]
    }

    fun colonMac(mac: String): String? = macBytes(mac)?.joinToString(":") { hex2(it) }

    /** Six 0xFF, then the MAC sixteen times. */
    fun packet(mac: ByteArray): ByteArray = ByteArray(6) { 0xFF.toByte() } + ByteArray(16 * mac.size) { mac[it % mac.size] }

    /** Sends the packet to each address on ports 9 and 7; how many went out. */
    fun sendMagic(mac: ByteArray, hosts: List<String>): Int {
        val bytes = packet(mac)
        var sent = 0
        DatagramSocket().use { socket ->
            socket.broadcast = true
            for (host in hosts.filter { it.isNotBlank() }) {
                val addr = try { InetAddress.getByName(host) } catch (_: Exception) { continue }
                for (port in intArrayOf(9, 7)) {
                    try {
                        socket.send(DatagramPacket(bytes, bytes.size, addr, port))
                        sent++
                    } catch (_: IOException) { }
                }
            }
        }
        return sent
    }

    private fun hex(b: ByteArray) = b.joinToString("") { hex2(it) }

    /** Keenetic's sign-in answer to its challenge: SHA-256(challenge + MD5(login:realm:password)), in hex. */
    fun keeneticPassword(login: String, realm: String, password: String, challenge: String): String {
        val md5 = hex(MessageDigest.getInstance("MD5").digest("$login:$realm:$password".toByteArray(Charsets.UTF_8)))
        return hex(MessageDigest.getInstance("SHA-256").digest((challenge + md5).toByteArray(Charsets.UTF_8)))
    }

    /** "192.168.1.1" → http (the router at home); a KeenDNS name → https. */
    fun base(address: String): String {
        var a = address.trim().trimEnd('/')
        if (!a.startsWith("http://", true) && !a.startsWith("https://", true)) {
            val host = a.substringBefore('/')
            val local = host.all { it.isDigit() || it == '.' || it == ':' } || host.endsWith(".local") || !host.contains('.')
            a = (if (local) "http://" else "https://") + a
        }
        return a
    }

    private class Session(val base: String) {
        var cookie: String? = null

        fun request(path: String, method: String = "GET", body: String? = null): Triple<Int, String, HttpURLConnection> {
            val c = try {
                (URL(base + path).openConnection() as HttpURLConnection).apply {
                    connectTimeout = 6_000
                    readTimeout = 8_000
                    requestMethod = method
                    instanceFollowRedirects = false
                    cookie?.let { setRequestProperty("Cookie", it) }
                    if (body != null) {
                        doOutput = true
                        setRequestProperty("Content-Type", "application/json")
                        outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
                    }
                }
            } catch (e: IOException) {
                throw RouterException(RouterProblem.UNREACHABLE, e.message ?: "")
            }
            val code = try { c.responseCode } catch (e: IOException) { throw RouterException(RouterProblem.UNREACHABLE, e.message ?: "") }
            c.headerFields["Set-Cookie"]?.firstOrNull()?.substringBefore(';')?.let { cookie = it }
            val text = try {
                (if (code >= 400) c.errorStream else c.inputStream)?.use { it.readBytes().take(256 * 1024).toByteArray().toString(Charsets.UTF_8) } ?: ""
            } catch (_: IOException) { "" }
            return Triple(code, text, c)
        }
    }

    private fun signIn(login: RouterLogin, address: String): Session {
        val s = Session(base(address))
        val (code, _, c) = s.request("/auth")
        if (code == 200) return s
        val realm = c.getHeaderField("X-NDM-Realm")
        val challenge = c.getHeaderField("X-NDM-Challenge")
        if (code != 401 || realm == null || challenge == null) throw RouterException(RouterProblem.NOT_KEENETIC)
        val body = JSONObject().put("login", login.login).put("password", keeneticPassword(login.login, realm, login.password, challenge)).toString()
        val (signed, _, _) = s.request("/auth", "POST", body)
        if (signed != 200) throw RouterException(RouterProblem.BAD_LOGIN)
        return s
    }

    /** Home first, then away; the first address that answers is used. */
    private fun <T> firstAnswering(login: RouterLogin, body: (String) -> T): T {
        var last = RouterException(RouterProblem.UNREACHABLE)
        for (address in login.addresses) {
            try {
                return body(address)
            } catch (e: RouterException) {
                if (e.problem != RouterProblem.UNREACHABLE) throw e
                last = e
            }
        }
        throw last
    }

    /** Checks the login; returns the address that answered. */
    fun testRouter(login: RouterLogin): String = firstAnswering(login) { address -> signIn(login, address); address }

    /** The router sends the magic packet itself. */
    fun routerWake(login: RouterLogin, mac: String) = firstAnswering(login) { address ->
        val s = signIn(login, address)
        val body = JSONObject().put("ip", JSONObject().put("hotspot", JSONObject().put("wake", JSONObject().put("mac", mac)))).toString()
        val (code, text, _) = s.request("/rci/", "POST", body)
        if (code != 200) throw RouterException(RouterProblem.REFUSED, "HTTP $code")
        if (text.contains("\"error\"")) throw RouterException(RouterProblem.REFUSED, text.take(160))
    }

    /** The computer's card from the router's list of devices, found by the computer's name (wired entries first). */
    fun routerFindMac(login: RouterLogin, computer: String): String? = firstAnswering(login) { address ->
        val s = signIn(login, address)
        val (code, text, _) = s.request("/rci/show/ip/hotspot")
        if (code != 200) throw RouterException(RouterProblem.REFUSED, "HTTP $code")
        pickMac(text, computer)
    }

    /** Of the router's `show ip hotspot` answer, the MAC of the host named like the computer; its wired entry first. */
    fun pickMac(json: String, computer: String): String? {
        val name = computer.trim().lowercase()
        if (name.isEmpty()) return null
        val hosts: JSONArray = try { JSONObject(json).optJSONArray("host") } catch (_: Exception) { null } ?: return null
        val named = (0 until hosts.length()).mapNotNull { hosts.optJSONObject(it) }
            .filter { h -> h.optString("hostname").lowercase() == name || h.optString("name").lowercase() == name }
        return named.sortedWith(compareBy<JSONObject>({ it.has("ssid") }, { !it.optBoolean("active") }))
            .map { it.optString("mac") }.firstOrNull { macBytes(it) != null }
    }
}
