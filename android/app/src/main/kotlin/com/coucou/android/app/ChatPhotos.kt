package com.coucou.android.app

import android.content.Context
import android.graphics.Bitmap
import android.graphics.ImageDecoder
import android.net.Uri
import com.coucou.android.link.Protocol
import java.io.ByteArrayOutputStream

/** A photo for the chat: a JPEG the computer takes (at most 1600 px a side), and a small preview. */
class ChatPhoto(val jpeg: ByteArray, val preview: Bitmap)

/** Photos are read and scaled here, off the main thread (as on the iPhone: LANChatPhoto). */
object ChatPhotos {
    private const val MAX_SIDE = 1600
    private const val PREVIEW_SIDE = 160

    fun fromUri(context: Context, uri: Uri): ChatPhoto? = try {
        val source = ImageDecoder.createSource(context.contentResolver, uri)
        val bitmap = ImageDecoder.decodeBitmap(source) { decoder, info, _ ->
            val w = info.size.width
            val h = info.size.height
            val longest = maxOf(w, h)
            if (longest > MAX_SIDE) {
                val k = MAX_SIDE.toDouble() / longest
                decoder.setTargetSize((w * k).toInt().coerceAtLeast(1), (h * k).toInt().coerceAtLeast(1))
            }
            decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
        }
        fromBitmap(bitmap)
    } catch (_: Exception) {
        null
    }

    fun fromBitmap(bitmap: Bitmap): ChatPhoto? {
        var quality = 75
        var jpeg = compress(bitmap, quality)
        // The computer takes 4 MB a photo.
        while (jpeg.size > Protocol.CHAT_MAX_IMAGE_BYTES * 7 / 8 && quality > 30) {
            quality -= 15
            jpeg = compress(bitmap, quality)
        }
        if (jpeg.isEmpty() || jpeg.size > Protocol.CHAT_MAX_IMAGE_BYTES) return null
        val longest = maxOf(bitmap.width, bitmap.height).coerceAtLeast(1)
        val k = minOf(1.0, PREVIEW_SIDE.toDouble() / longest)
        val preview = Bitmap.createScaledBitmap(bitmap, (bitmap.width * k).toInt().coerceAtLeast(1), (bitmap.height * k).toInt().coerceAtLeast(1), true)
        return ChatPhoto(jpeg, preview)
    }

    private fun compress(bitmap: Bitmap, quality: Int): ByteArray {
        val out = ByteArrayOutputStream()
        bitmap.compress(Bitmap.CompressFormat.JPEG, quality, out)
        return out.toByteArray()
    }
}
