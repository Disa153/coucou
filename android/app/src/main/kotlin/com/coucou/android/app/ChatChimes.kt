package com.coucou.android.app

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import kotlin.concurrent.thread
import kotlin.math.PI
import kotlin.math.exp
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sin

/**
 * The beta chat's sound for a new message, made here sample by sample (none is a recording), the same
 * five as on the computer: a falsetto "hee" in the spirit of the King of Pop, three notes like a phone's
 * message, a quack, three knocks on a door, Mochi's blip. Played once through a short static AudioTrack.
 */
object ChatChimes {
    val KINDS = listOf("chime", "hee", "quack", "knock", "mochi", "none")
    private const val RATE = 44_100

    private class Buf(seconds: Double) {
        val s = FloatArray((seconds * RATE).toInt())
        /** A tone from [t0] for [d] s, its pitch gliding from [f0] to [f1], with a quick attack and a decay. */
        fun tone(f0: Double, t0: Double, d: Double, vol: Double, f1: Double = f0, shape: (Double) -> Double = ::sine, vibrato: Double = 0.0) {
            val start = (t0 * RATE).toInt()
            val n = (d * RATE).toInt()
            var phase = 0.0
            for (i in 0 until n) {
                val k = start + i
                if (k >= s.size) break
                val t = i.toDouble() / RATE
                val f = f0 * Math.pow(f1 / f0, t / d) + vibrato * sin(2 * PI * 9 * t)
                phase += 2 * PI * f / RATE
                val env = min(1.0, t / 0.012) * exp(-5.0 * t / d)
                s[k] += (shape(phase) * env * vol).toFloat()
            }
        }
    }

    private fun sine(p: Double) = sin(p)
    private fun triangle(p: Double): Double { val x = (p / (2 * PI)) % 1.0; return 4 * kotlin.math.abs(x - 0.5) - 1 }
    private fun saw(p: Double): Double = 2 * ((p / (2 * PI)) % 1.0) - 1

    /** The samples of [kind] (−1…1), or null for "none". */
    fun samples(kind: String): FloatArray? {
        val b: Buf
        when (kind) {
            "hee" -> {
                b = Buf(0.8)
                b.tone(700.0, 0.0, 0.34, 0.30, 1500.0, ::triangle, vibrato = 40.0)
                b.tone(1300.0, 0.37, 0.22, 0.24, 1700.0, ::triangle)
                b.tone(1700.0, 0.60, 0.12, 0.16, 1200.0)
            }
            "chime" -> {
                b = Buf(1.0)
                b.tone(1318.5, 0.0, 0.35, 0.26)
                b.tone(1046.5, 0.14, 0.35, 0.26)
                b.tone(1568.0, 0.28, 0.6, 0.26)
            }
            "quack" -> {
                b = Buf(0.5)
                // A buzzy tone with a falling pitch: crude, but a duck.
                b.tone(260.0, 0.0, 0.17, 0.35, 180.0, ::saw)
                b.tone(260.0, 0.22, 0.17, 0.35, 180.0, ::saw)
            }
            "knock" -> {
                b = Buf(0.55)
                val rnd = java.util.Random(7)
                for (t0 in doubleArrayOf(0.0, 0.18, 0.36)) {
                    val start = (t0 * RATE).toInt()
                    var low = 0.0
                    for (i in 0 until (0.08 * RATE).toInt()) {
                        val k = start + i
                        if (k >= b.s.size) break
                        val noise = (rnd.nextDouble() * 2 - 1) * exp(-i / (RATE * 0.012))
                        low += (noise - low) * 0.08 // a soft low-pass: wood, not a click
                        b.s[k] += (low * 2.2).toFloat()
                    }
                    b.tone(110.0, t0, 0.09, 0.5)
                }
            }
            "mochi" -> {
                b = Buf(0.2)
                b.tone(880.0, 0.0, 0.12, 0.25, 1320.0)
            }
            else -> return null
        }
        return b.s
    }

    /** Plays [kind] at [volume] (0…1) on its own thread; nothing for "none" or volume 0. */
    fun play(kind: String, volume: Float) {
        if (volume <= 0f) return
        val samples = samples(kind) ?: return
        thread(name = "coucou-chime", isDaemon = true) {
            runCatching {
                val pcm = ShortArray(samples.size) { i -> (max(-1f, min(1f, samples[i] * volume)) * Short.MAX_VALUE).toInt().toShort() }
                val track = AudioTrack.Builder()
                    .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_NOTIFICATION).setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build())
                    .setAudioFormat(AudioFormat.Builder().setSampleRate(RATE).setEncoding(AudioFormat.ENCODING_PCM_16BIT).setChannelMask(AudioFormat.CHANNEL_OUT_MONO).build())
                    .setTransferMode(AudioTrack.MODE_STATIC)
                    .setBufferSizeInBytes(pcm.size * 2)
                    .build()
                track.write(pcm, 0, pcm.size)
                track.play()
                Thread.sleep(pcm.size * 1000L / RATE + 100)
                track.release()
            }
        }
    }
}
