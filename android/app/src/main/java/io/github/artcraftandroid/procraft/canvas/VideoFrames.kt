package io.github.artcraftandroid.procraft.canvas

import android.graphics.Bitmap
import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaFormat
import io.github.artcraftandroid.procraft.AppLog
import java.io.File

/**
 * A video's frames read back with the device's decoder, by their number in the order they show:
 * the frames after the last one read come from the decoder going on (a replay or a check reads
 * them in order), any other from the key frame before it. Each frame's YUV is read as the decoder
 * lays it out (its crop, strides and slice height), so nothing is left to a reader that may get a
 * device's layout wrong.
 */
class VideoFrames private constructor(
    private val extractor: MediaExtractor,
    private val codec: MediaCodec,
    /** Each frame's time, in the order they show. */
    private val times: LongArray,
    /** How the video's YUV becomes RGB, as it says (BT.601, video range, where it says nothing). */
    private val matrix: Yuv.Matrix,
    /** The format the decoder was set up with (its codec data in it). */
    private val format: MediaFormat,
) : AutoCloseable {
    val count: Int get() = times.size

    /** The frame the decoder gives next, while it goes on from the last one read; -1 when it must seek. */
    private var next = -1
    private var inputDone = false
    /** Samples given since the decoder started or was flushed, and whether it has said its format since starting. */
    private var given = false
    private var said = false
    private var outputFormat: MediaFormat? = null
    private val info = MediaCodec.BufferInfo()

    /**
     * Frame [i]'s picture handed to [use] (while the decoder holds it); null when there is no such
     * frame or the decoder gives none (a broken video, a decoder that stops). Throws where the
     * decoder can't be used here (then a reader of the system's is left).
     */
    fun <T> read(i: Int, use: (YuvPicture, Yuv.Matrix) -> T): T? {
        if (i !in times.indices) return null
        if (next < 0 || i < next || i - next > GOING_ON_MOST) seek(i)
        val target = times[i]
        val until = System.nanoTime() + READ_MOST_NS
        while (System.nanoTime() < until) {
            feed()
            val index = codec.dequeueOutputBuffer(info, TIMEOUT_US)
            when {
                index == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> {
                    outputFormat = codec.outputFormat
                    said = true
                }
                index >= 0 -> {
                    said = true
                    val end = info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0
                    val at = info.presentationTimeUs
                    if (info.size <= 0 && end) {
                        codec.releaseOutputBuffer(index, false)
                        next = -1
                        return null
                    }
                    if (at < target) {
                        codec.releaseOutputBuffer(index, false)
                        if (end) {
                            next = -1
                            return null
                        }
                        continue
                    }
                    try {
                        // (a frame missing: the one after it is no stand-in for it)
                        if (at != target) return null
                        val picture = picture(index) ?: throw IllegalStateException("the decoder's frames are laid out as this can't read")
                        return use(picture, matrix)
                    } finally {
                        codec.releaseOutputBuffer(index, false)
                        val shown = times.binarySearch(at)
                        next = if (end || shown < 0) -1 else shown + 1
                    }
                }
            }
        }
        AppLog.w("HanaCreate", "a video's frame $i took longer than ${READ_MOST_NS / 1_000_000} ms to decode")
        next = -1
        return null
    }

    /**
     * The decoder taken back to the key frame at or before frame [i]: flushed, unless nothing was
     * given it yet; started afresh where it was given samples but gave nothing back yet (a flush
     * then would lose the codec data its format gave, and it would decode garbage).
     */
    private fun seek(i: Int) {
        extractor.seekTo(times[i], MediaExtractor.SEEK_TO_PREVIOUS_SYNC)
        if (given && said) {
            codec.flush()
        } else if (given) {
            codec.stop()
            codec.configure(format, null, null, 0)
            codec.start()
            said = false
        }
        given = false
        inputDone = false
        next = i
    }

    /** The video's samples given to the decoder while it takes them. */
    private fun feed() {
        while (!inputDone) {
            val index = codec.dequeueInputBuffer(0)
            if (index < 0) return
            val buffer = codec.getInputBuffer(index) ?: return
            val size = extractor.readSampleData(buffer, 0)
            given = true
            if (size < 0) {
                codec.queueInputBuffer(index, 0, 0, 0, MediaCodec.BUFFER_FLAG_END_OF_STREAM)
                inputDone = true
            } else {
                codec.queueInputBuffer(index, 0, size, extractor.sampleTime, 0)
                extractor.advance()
            }
        }
    }

    /** Output buffer [index]'s picture: as the decoder's image says, else as its format says. */
    private fun picture(index: Int): YuvPicture? {
        codec.getOutputImage(index)?.let { image ->
            // (more than 8 bits a sample: left to the system's reader)
            if (image.format != android.graphics.ImageFormat.YUV_420_888) return null
            val planes = image.planes
            if (planes.size < 3) return null
            val crop = image.cropRect
            val plane = { k: Int -> YuvPlane(planes[k].buffer, planes[k].rowStride, planes[k].pixelStride) }
            return YuvPicture(plane(0), plane(1), plane(2), crop.left, crop.top, crop.width(), crop.height())
        }
        val f = outputFormat ?: codec.outputFormat
        val buffer = codec.getOutputBuffer(index) ?: return null
        val int = { key: String, or: Int -> if (f.containsKey(key)) f.getInteger(key) else or }
        val width = int(MediaFormat.KEY_WIDTH, 0)
        val height = int(MediaFormat.KEY_HEIGHT, 0)
        val right = int("crop-right", width - 1)
        val bottom = int("crop-bottom", height - 1)
        return Yuv.planesIn(
            buffer, int(MediaFormat.KEY_COLOR_FORMAT, 0), right + 1, bottom + 1,
            int(MediaFormat.KEY_STRIDE, width), int(MediaFormat.KEY_SLICE_HEIGHT, height),
        )
    }

    override fun close() {
        runCatching { codec.stop() }
        runCatching { codec.release() }
        runCatching { extractor.release() }
    }

    /** What reading a frame came to: the picture, a frame the decoder left unfilled, or none. */
    sealed interface Frame {
        class Shown(val bitmap: Bitmap) : Frame
        data object Unfilled : Frame
        data object None : Frame
    }

    /**
     * Frame [i] as a bitmap; [Frame.Unfilled] when the decoder left any of it unfilled (green on
     * screen: never shown, the one before stays).
     */
    fun bitmap(i: Int): Frame = read(i) { p, m ->
        if (Yuv.unfilledShare(p) > Yuv.UNFILLED_MOST) {
            Frame.Unfilled
        } else {
            val argb = IntArray(p.width * p.height)
            Yuv.toArgb(p, argb, m)
            Frame.Shown(Bitmap.createBitmap(argb, p.width, p.height, Bitmap.Config.ARGB_8888))
        }
    } ?: Frame.None

    /** What checking a video's frames against what it was made of found. */
    sealed interface Checked {
        data object Right : Checked
        /** Frame [frame] isn't what it was made of (or isn't there). */
        data class Wrong(val frame: Int) : Checked
        /** The video can't be read here, so it isn't known. */
        data object Unknown : Checked
    }

    companion object {
        private const val TIMEOUT_US = 5_000L
        /** The longest one frame's decoding may take. */
        private const val READ_MOST_NS = 4_000_000_000L
        /** Frames the decoder goes on through rather than seek (about two seconds of a time-lapse). */
        private const val GOING_ON_MOST = 60

        /** The video at [path] opened for reading, or null where it can't be (no video, no decoder). */
        fun open(path: String): VideoFrames? {
            val extractor = MediaExtractor()
            var codec: MediaCodec? = null
            try {
                extractor.setDataSource(path)
                val track = (0 until extractor.trackCount).firstOrNull {
                    extractor.getTrackFormat(it).getString(MediaFormat.KEY_MIME)?.startsWith("video/") == true
                } ?: return null.also { extractor.release() }
                val format = extractor.getTrackFormat(track)
                extractor.selectTrack(track)
                // every frame's time, in the order they show
                val times = ArrayList<Long>()
                while (true) {
                    val t = extractor.sampleTime
                    if (t < 0) break
                    times.add(t)
                    if (!extractor.advance()) break
                }
                val shown = times.toLongArray().also { it.sort() }
                extractor.seekTo(0, MediaExtractor.SEEK_TO_PREVIOUS_SYNC)
                val standard = if (format.containsKey(MediaFormat.KEY_COLOR_STANDARD)) format.getInteger(MediaFormat.KEY_COLOR_STANDARD) else 0
                val range = if (format.containsKey(MediaFormat.KEY_COLOR_RANGE)) format.getInteger(MediaFormat.KEY_COLOR_RANGE) else 0
                val matrix = Yuv.Matrix(bt709 = standard == MediaFormat.COLOR_STANDARD_BT709, full = range == MediaFormat.COLOR_RANGE_FULL)
                val mime = format.getString(MediaFormat.KEY_MIME) ?: return null.also { extractor.release() }
                format.setInteger(MediaFormat.KEY_COLOR_FORMAT, Yuv.FLEXIBLE)
                codec = MediaCodec.createDecoderByType(mime)
                codec.configure(format, null, null, 0)
                codec.start()
                return VideoFrames(extractor, codec, shown, matrix, format)
            } catch (e: Exception) {
                AppLog.w("HanaCreate", "a video can't be decoded here: ${File(path).name}", e)
                runCatching { codec?.release() }
                extractor.release()
                return null
            }
        }

        /**
         * Whether every frame of the video at [path] shows what [looks] (each frame's [Yuv.cells], in
         * order) says it was made of: as many, none left unfilled, none apart from its own. [going]
         * hears how far it has come (0..1) and stops it (then [Checked.Unknown]).
         */
        fun check(path: String, looks: List<ByteArray>, going: (Float) -> Boolean = { true }): Checked {
            val frames = open(path) ?: return Checked.Unknown
            return try {
                if (frames.count != looks.size) return Checked.Wrong(minOf(frames.count, looks.size))
                for (i in looks.indices) {
                    if (!going(i / looks.size.toFloat())) return Checked.Unknown
                    val right = frames.read(i) { p, m ->
                        Yuv.unfilledShare(p) <= Yuv.UNFILLED_MOST && !Yuv.apart(Yuv.cells(p, m), looks[i])
                    }
                    if (right != true) return Checked.Wrong(i)
                }
                Checked.Right
            } catch (e: Exception) {
                AppLog.w("HanaCreate", "checking a video", e)
                Checked.Unknown
            } finally {
                frames.close()
            }
        }
    }
}
