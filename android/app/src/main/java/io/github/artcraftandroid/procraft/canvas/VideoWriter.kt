package io.github.artcraftandroid.procraft.canvas

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaMuxer
import io.github.artcraftandroid.procraft.AppLog
import java.io.File
import java.nio.ByteBuffer
import kotlin.math.roundToInt

/** Video files made a frame at a time with the device's encoder. */
object VideoWriter {
    private const val BITS_PER_PIXEL = 4
    private const val TIMEOUT_US = 10_000L

    /**
     * The largest size no bigger than [width] × [height] (same shape, sides multiples of [align],
     * longest side at most [maxSide]) the encoder for [mime] takes; null if there is no such encoder.
     * [align] 16 gives sizes every codec lays out in whole blocks, with no padding or crop for a
     * device's codec to get wrong (a frame is then fitted into it, a little white either side).
     */
    fun fit(width: Int, height: Int, maxSide: Int, mime: String = MediaFormat.MIMETYPE_VIDEO_AVC, align: Int = 2): Pair<Int, Int>? {
        val caps = MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos
            .filter { it.isEncoder && it.supportedTypes.any { t -> t.equals(mime, ignoreCase = true) } }
            .mapNotNull { runCatching { it.getCapabilitiesForType(mime).videoCapabilities }.getOrNull() }
        if (caps.isEmpty()) return null.also { AppLog.w("HanaCreate", "this device has no $mime encoder") }
        var scale = minOf(1f, maxSide / maxOf(width, height).toFloat())
        repeat(40) {
            val (w, h) = aligned(width * scale, height * scale, align)
            if (caps.any { it.isSizeSupported(w, h) }) return w to h
            scale *= 0.9f
        }
        return null.also { AppLog.w("HanaCreate", "no $mime encoder here takes a ${width}x$height video made smaller (${sizesTaken(mime)})") }
    }

    /** The sizes each encoder for [mime] takes, for the log: "name: widths x heights". */
    private fun sizesTaken(mime: String): String =
        MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos
            .filter { it.isEncoder && it.supportedTypes.any { t -> t.equals(mime, ignoreCase = true) } }
            .joinToString("; ") { info ->
                val caps = runCatching { info.getCapabilitiesForType(mime).videoCapabilities }.getOrNull()
                "${info.name}: ${caps?.supportedWidths ?: "?"} x ${caps?.supportedHeights ?: "?"}"
            }
            .ifEmpty { "none" }

    /** [width] × [height] rounded down to multiples of [align] (2 at least), never below one [align]. */
    fun aligned(width: Float, height: Float, align: Int): Pair<Int, Int> {
        val a = align.coerceAtLeast(2)
        return ((width.toInt() / a * a).coerceAtLeast(a)) to ((height.toInt() / a * a).coerceAtLeast(a))
    }

    /**
     * [count] frames, [width] × [height] (even), [fps] a second (a fraction of a frame too, to a
     * thousandth, each frame's time exact), as an MP4 at [out] encoded with [mime]. [frame] fills a
     * frame's ARGB pixels (rows top to bottom) and says whether it has one. Each encoder that takes
     * the size is tried in the system's order (a tablet's own may refuse a size its software one
     * takes); false if none makes it.
     */
    fun write(
        out: File, width: Int, height: Int, fps: Float, count: Int, mime: String = MediaFormat.MIMETYPE_VIDEO_AVC,
        bitsPerPixel: Int = BITS_PER_PIXEL, frame: (Int, IntArray) -> Boolean,
    ): Boolean = make(out, width, height, fps, count, mime, bitsPerPixel, check = false, frame = frame) != Made.NONE

    /** What [make] made: nothing, a video read back right, or one that couldn't be (or wasn't asked to be). */
    enum class Made { NONE, CHECKED, UNCHECKED }

    /**
     * As [write]; with [check], each video made is read back, and one whose frames don't show what
     * they were given (an encoder that leaves some of a frame unfilled, green) goes and the next
     * encoder makes it. Where none reads back right, the first one made is kept ([Made.UNCHECKED]:
     * the reading back may be what is wrong on this device). [only] names the one encoder to try.
     */
    fun make(
        out: File, width: Int, height: Int, fps: Float, count: Int, mime: String = MediaFormat.MIMETYPE_VIDEO_AVC,
        bitsPerPixel: Int = BITS_PER_PIXEL, check: Boolean, only: String? = null, frame: (Int, IntArray) -> Boolean,
    ): Made {
        var first: File? = null
        val takers = encoders(mime, width, height).filter { only == null || it.name == only }
        if (takers.isEmpty()) AppLog.w("HanaCreate", "no $mime encoder here takes a ${width}x$height video (${sizesTaken(mime)})")
        for (info in takers) {
            val looks = if (check) ArrayList<ByteArray>(count) else null
            if (!writeWith(info, out, width, height, fps, count, mime, bitsPerPixel, frame, looks)) {
                out.delete()
                continue
            }
            // (one cut short, stopped, is its maker's to deal with)
            if (looks == null || looks.size < count) return Made.UNCHECKED.also { first?.delete() }
            when (val checked = VideoFrames.check(out.path, looks)) {
                VideoFrames.Checked.Right -> return Made.CHECKED.also { first?.delete() }
                VideoFrames.Checked.Unknown -> return Made.UNCHECKED.also { first?.delete() }
                is VideoFrames.Checked.Wrong -> {
                    AppLog.w("HanaCreate", "${info.name}'s ${width}x$height video shows frame ${checked.frame} of ${looks.size} otherwise than it was given: made again")
                    if (first == null) first = File(out.path + ".first").also { if (!out.renameTo(it)) out.delete() } else out.delete()
                }
            }
        }
        val kept = first ?: return Made.NONE.also {
            if (takers.isNotEmpty()) AppLog.w("HanaCreate", "no encoder made the ${width}x$height $mime video (${takers.joinToString { it.name }} tried)")
        }
        return if (kept.renameTo(out)) Made.UNCHECKED else Made.NONE.also {
            AppLog.w("HanaCreate", "the ${width}x$height video made couldn't be put in place at ${out.name}")
            kept.delete()
        }
    }

    /** The names of the encoders for [mime] that take [width] × [height], in the system's order. */
    fun encoderNames(mime: String, width: Int, height: Int): List<String> = encoders(mime, width, height).map { it.name }

    /** The encoders for [mime] that take [width] × [height], in the system's order. */
    private fun encoders(mime: String, width: Int, height: Int): List<MediaCodecInfo> =
        MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos
            .filter { it.isEncoder && it.supportedTypes.any { t -> t.equals(mime, ignoreCase = true) } }
            .filter { runCatching { it.getCapabilitiesForType(mime).videoCapabilities?.isSizeSupported(width, height) == true }.getOrDefault(false) }

    /** When frame [n] starts at [fps] frames a second, in microseconds: exact for a rate in thousandths. */
    fun frameTime(n: Int, fps: Float): Long {
        val rate = io.github.artcraftandroid.procraft.core.frameRate(fps)
        return n * 1_000_000L * rate.seconds.toLong() / rate.frames.toLong().coerceAtLeast(1)
    }

    /**
     * The video made with encoder [info]; with [looks], each frame's [Yuv.cells] added to it as it is
     * given. Its input as the encoder lists it takes (flexible where it does), each frame written
     * into the picture the encoder hands over, else into its buffer as the encoder's stride and slice
     * height lay it out, and the whole buffer given back.
     */
    private fun writeWith(
        info: MediaCodecInfo, out: File, width: Int, height: Int, fps: Float, count: Int, mime: String, bitsPerPixel: Int,
        frame: (Int, IntArray) -> Boolean, looks: MutableList<ByteArray>?,
    ): Boolean {
        val listed = runCatching { info.getCapabilitiesForType(mime).colorFormats }.getOrNull() ?: IntArray(0)
        val input = Yuv.inputFormat(listed) ?: Yuv.FLEXIBLE
        val format = MediaFormat.createVideoFormat(mime, width, height).apply {
            setInteger(MediaFormat.KEY_COLOR_FORMAT, input)
            setInteger(MediaFormat.KEY_BIT_RATE, bitRate(width, height, bitsPerPixel))
            // the encoder's guide to its bits a frame, whole (encoders may refuse a rate under one);
            // the frames' times say how long each shows
            setInteger(MediaFormat.KEY_FRAME_RATE, fps.roundToInt().coerceAtLeast(1))
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1)
        }
        // each way it can fail said in the log, with the encoder, the size and the step it got to
        val what = "${info.name}'s ${width}x$height $mime video"
        val codec = try {
            MediaCodec.createByCodecName(info.name)
        } catch (e: Exception) {
            AppLog.w("HanaCreate", "$what: the encoder couldn't be made", e)
            return false
        }
        val muxer = try {
            MediaMuxer(out.path, MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4)
        } catch (e: Exception) {
            AppLog.w("HanaCreate", "$what: its file ${out.name} couldn't be begun", e)
            codec.release()
            return false
        }
        var track = -1
        var stopped = false
        val done = MediaCodec.BufferInfo()
        val pixels = IntArray(width * height)
        var step = "configured"
        // an encoder that stops taking frames or giving them back is given up on, not waited for
        // for ever (the export's card up all the while)
        val patience = Patience()
        try {
            codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
            step = "started"
            codec.start()
            fun drain(end: Boolean) {
                while (true) {
                    val index = codec.dequeueOutputBuffer(done, if (end) TIMEOUT_US else 0)
                    when {
                        index == MediaCodec.INFO_TRY_AGAIN_LATER -> {
                            if (!end) return
                            if (patience.spent()) throw IllegalStateException("no end of its stream for ${Patience.MOST_MS} ms")
                        }
                        index == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> {
                            track = muxer.addTrack(codec.outputFormat)
                            muxer.start()
                        }
                        index >= 0 -> {
                            patience.progressed()
                            val buffer = codec.getOutputBuffer(index)
                            if (buffer != null && done.size > 0 && track >= 0 && done.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG == 0) {
                                muxer.writeSampleData(track, buffer, done)
                            }
                            codec.releaseOutputBuffer(index, false)
                            if (done.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) return
                        }
                    }
                }
            }
            fun nextInput(): Int {
                // (the wait for it counted, not the frame's making before)
                patience.progressed()
                var input: Int
                do {
                    input = codec.dequeueInputBuffer(TIMEOUT_US)
                    if (input < 0) {
                        drain(false)
                        if (patience.spent()) throw IllegalStateException("no input buffer for ${Patience.MOST_MS} ms")
                    }
                } while (input < 0)
                patience.progressed()
                return input
            }
            var n = 0
            for (i in 0 until count) {
                step = "frame $i of $count"
                if (!frame(i, pixels)) continue
                looks?.add(Yuv.cells(pixels, width, height))
                val index = nextInput()
                // the whole buffer is given back: a codec padding its rows or planes reads all of
                // it, and one given less may leave the rest of the frame unfilled (green)
                val size = codec.getInputBuffer(index)?.capacity()
                    ?: return false.also { AppLog.w("HanaCreate", "$what: no input buffer to write $step into") }
                // without a picture to fill, the buffer could never be given back
                val picture = codec.getInputImage(index)?.let { image ->
                    val planes = image.planes
                    if (planes.size < 3) return false.also { AppLog.w("HanaCreate", "$what: its input picture has ${planes.size} planes ($step)") }
                    val plane = { k: Int -> YuvPlane(planes[k].buffer, planes[k].rowStride, planes[k].pixelStride) }
                    YuvPicture(plane(0), plane(1), plane(2), 0, 0, width, height)
                } ?: run {
                    val f = codec.inputFormat
                    val int = { key: String -> if (f.containsKey(key)) f.getInteger(key) else 0 }
                    val buffer = codec.getInputBuffer(index) ?: return false.also { AppLog.w("HanaCreate", "$what: no input buffer to write $step into") }
                    val laid = if (f.containsKey(MediaFormat.KEY_COLOR_FORMAT)) f.getInteger(MediaFormat.KEY_COLOR_FORMAT) else input
                    Yuv.planesIn(buffer, laid, width, height, int(MediaFormat.KEY_STRIDE), int(MediaFormat.KEY_SLICE_HEIGHT))
                } ?: return false.also { AppLog.w("HanaCreate", "$what: it lays its input out as this can't write ($input)") }
                Yuv.write(pixels, width, height, picture)
                codec.queueInputBuffer(index, 0, size, frameTime(n, fps), 0)
                n++
                drain(false)
            }
            step = "ended after $n frames"
            codec.queueInputBuffer(nextInput(), 0, 0, frameTime(n, fps), MediaCodec.BUFFER_FLAG_END_OF_STREAM)
            patience.progressed()
            drain(true)
            if (track < 0) AppLog.w("HanaCreate", "$what: the encoder gave nothing back ($step)")
            // its file ended here: one whose end couldn't be written (the storage full) is no video
            if (track >= 0) {
                step = "its file ended"
                muxer.stop()
                stopped = true
            }
            return track >= 0
        } catch (e: Exception) {
            AppLog.w("HanaCreate", "$what failed, $step", e)
            return false
        } finally {
            runCatching { codec.stop() }
            codec.release()
            // a stop that failed leaves Android 8 and 9's muxer started, and its release then stops
            // it again and throws
            runCatching { if (track >= 0 && !stopped) muxer.stop() }
            runCatching { muxer.release() }
        }
    }

    /**
     * Videos [parts] (made alike: one video track each, the same kind, size and stream settings) one
     * after another at [out], their frames copied, nothing encoded again; each part's frames follow
     * the last one's by one frame at [fps]. False if a part can't be read or was made otherwise.
     * One part is copied as it is, needing nothing of the device's media system.
     */
    fun join(parts: List<File>, out: File, fps: Float): Boolean {
        parts.singleOrNull()?.let { part -> return runCatching { part.copyTo(out, overwrite = true) }.isSuccess }
        val muxer = runCatching { MediaMuxer(out.path, MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4) }.getOrNull() ?: return false
        var track = -1
        var stopped = false
        var first: MediaFormat? = null
        var offset = 0L
        var buffer = ByteBuffer.allocate(0)
        val info = MediaCodec.BufferInfo()
        try {
            for (part in parts) {
                val extractor = MediaExtractor()
                try {
                    extractor.setDataSource(part.path)
                    val index = (0 until extractor.trackCount).firstOrNull {
                        extractor.getTrackFormat(it).getString(MediaFormat.KEY_MIME)?.startsWith("video/") == true
                    } ?: return false
                    val format = extractor.getTrackFormat(index)
                    val lead = first
                    if (lead == null) {
                        first = format
                        track = muxer.addTrack(format)
                        muxer.start()
                    } else if (!madeAlike(lead, format)) {
                        return false
                    }
                    val most = maxOf(
                        if (format.containsKey(MediaFormat.KEY_MAX_INPUT_SIZE)) format.getInteger(MediaFormat.KEY_MAX_INPUT_SIZE) else 0,
                        format.getInteger(MediaFormat.KEY_WIDTH) * format.getInteger(MediaFormat.KEY_HEIGHT) * 3 / 2,
                    )
                    if (buffer.capacity() < most) buffer = ByteBuffer.allocate(most)
                    extractor.selectTrack(index)
                    var last = -1L
                    while (true) {
                        val size = extractor.readSampleData(buffer, 0)
                        if (size < 0) break
                        val key = extractor.sampleFlags and MediaExtractor.SAMPLE_FLAG_SYNC != 0
                        info.set(0, size, offset + extractor.sampleTime, if (key) MediaCodec.BUFFER_FLAG_KEY_FRAME else 0)
                        muxer.writeSampleData(track, buffer, info)
                        last = maxOf(last, extractor.sampleTime)
                        extractor.advance()
                    }
                    if (last < 0) return false
                    offset += last + frameTime(1, fps)
                } finally {
                    extractor.release()
                }
            }
            // its file ended here: one whose end couldn't be written (the storage full) is no video
            if (track >= 0) {
                muxer.stop()
                stopped = true
            }
            return track >= 0
        } catch (e: Exception) {
            AppLog.e("HanaCreate", "joining videos", e)
            return false
        } finally {
            runCatching { if (track >= 0 && !stopped) muxer.stop() }
            runCatching { muxer.release() }
        }
    }

    /** Whether two videos were made the same way: the same kind and size, and the same stream settings. */
    private fun madeAlike(a: MediaFormat, b: MediaFormat): Boolean {
        val same = { key: String -> a.containsKey(key) == b.containsKey(key) }
        return a.getString(MediaFormat.KEY_MIME) == b.getString(MediaFormat.KEY_MIME) &&
            a.getInteger(MediaFormat.KEY_WIDTH) == b.getInteger(MediaFormat.KEY_WIDTH) &&
            a.getInteger(MediaFormat.KEY_HEIGHT) == b.getInteger(MediaFormat.KEY_HEIGHT) &&
            listOf("csd-0", "csd-1", "csd-2").all { key -> same(key) && (!a.containsKey(key) || a.getByteBuffer(key) == b.getByteBuffer(key)) }
    }

    /**
     * How long an encoder may go without taking a frame or giving one back before it is given up on:
     * [mostMs] since it last did, on the clock [now] (milliseconds).
     */
    internal class Patience(private val mostMs: Long = MOST_MS, private val now: () -> Long = { System.nanoTime() / 1_000_000 }) {
        private var since = now()

        /** It took a frame or gave one back: the wait starts again. */
        fun progressed() {
            since = now()
        }

        /** It has been waited for longer than the most since it last did. */
        fun spent(): Boolean = now() - since > mostMs

        companion object {
            const val MOST_MS = 10_000L
        }
    }

    /** The bit rate a [width] × [height] video is encoded at, bits a second. */
    fun bitRate(width: Int, height: Int, bitsPerPixel: Int = BITS_PER_PIXEL): Int = (width * height * bitsPerPixel).coerceAtLeast(1_000_000)
}
