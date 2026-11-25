package io.github.artcraftandroid.procraft.canvas

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Rect
import io.github.artcraftandroid.procraft.AppLog
import io.github.artcraftandroid.procraft.core.PackageTimeLapse
import io.github.artcraftandroid.procraft.core.timeLapseBitsPerPixel
import io.github.artcraftandroid.procraft.core.timeLapseFps
import io.github.artcraftandroid.procraft.core.timeLapsePicks
import io.github.artcraftandroid.procraft.core.timeLapseSide
import java.io.File
import java.nio.ByteBuffer
import io.github.artcraftandroid.procraft.core.TimeLapse as CoreTimeLapse
import io.github.artcraftandroid.procraft.core.TimeLapseSettings as CoreTimeLapseSettings

/**
 * A time-lapse's video: 1080p, 2K or 4K ([resolution] 0 to 2), Low, Good, Studio quality or
 * Lossless ([quality] 0 to 3), and HEVC or H.264.
 */
data class TimeLapseSettings(val resolution: Int = 0, val quality: Int = 1, val hevc: Boolean = false) {
    /** As the shared core takes them. */
    fun core() = CoreTimeLapseSettings(resolution.toUInt(), quality.toUInt(), hevc)

    /** The frames' longest side, pixels. */
    val side: Int get() = timeLapseSide(core()).toInt()
    val bitsPerPixel: Int get() = timeLapseBitsPerPixel(core()).toInt()
}

/**
 * An artwork's time-lapse: a picture of the whole canvas after each change, kept as numbered JPEG
 * frames in the artwork's folder, replayed or made into a video at [FPS] frames a second. The
 * shared core keeps the frames, the settings and a package's videos (crates/core timelapse.rs);
 * this makes and reads the videos with the system's codecs.
 *
 * As the artwork file keeps a time-lapse, its frames end up in videos only: a package's come in as they
 * are ([take]), the frames recorded become one more when the artwork is left or shared
 * ([forPackage]), their pictures then gone; [frame] reads those from the videos.
 */
class TimeLapse(artwork: File) {
    private val core = CoreTimeLapse(artwork.path)

    /** The video's size, quality and codec, kept with the artwork. */
    var settings: TimeLapseSettings
        get() = core.settings().let { TimeLapseSettings(it.resolution.toInt(), it.quality.toInt(), it.hevc) }
        set(v) = core.setSettings(v.core())

    /** Frames recorded so far. */
    val count: Int get() = core.count().toInt()

    /** Whether changes are recorded (on unless turned off for this artwork). */
    var recording: Boolean
        get() = core.recording()
        set(on) = core.setRecording(on)

    /**
     * Keeps [session]'s canvas as the next frame, unless it is the same as the last; the picture never leaves the core.
     * While the canvas is drawn, the frame that has laid the change takes it and this returns at once; whether one was
     * asked for or kept.
     */
    fun recordFrom(session: io.github.artcraftandroid.procraft.core.CanvasSession): Boolean =
        session.recordTimeLapse(core)

    /** What to tell, once, when the time-lapse couldn't be written (the storage full, say); null otherwise. */
    fun takeProblem(): String? = core.takeProblem()

    /** Deletes what has been recorded; the settings, and whether it records, stay. */
    fun purge() {
        core.purge()
        forgetClips()
    }

    /** What the recorded frames take on disk, bytes. */
    fun bytes(): Long = core.bytes().toLong()

    /** Frame [i]: its picture, or read from the video it is in. */
    fun frame(i: Int): Bitmap? =
        core.frameFile(i.toUInt())?.let { f -> runCatching { BitmapFactory.decodeFile(f) }.getOrNull() } ?: clipFrame(i)

    /** Whether frame [i] is kept (a picture, or in a video): one that can't be read but is, this device can't decode. */
    fun kept(i: Int): Boolean = core.frameKept(i.toUInt())

    /** Frames the last video made couldn't read, the one before each shown in its place. */
    @Volatile
    var unread = 0
        private set

    private val clipLock = Any()
    /** The videos the frames are in, the one being read, and the frames last read from it (a few at a time, the replay and the export reading them in order): the first one's number and them. */
    private var clips: List<io.github.artcraftandroid.procraft.core.TimeLapseClip>? = null
    private var reading: Pair<String, android.media.MediaMetadataRetriever>? = null
    private var decoded: Pair<Int, List<Bitmap>>? = null
    /** The video being read with the device's decoder, and the videos it can't read here (the system's reader reads them). */
    private var decoding: Pair<String, VideoFrames>? = null
    private val undecodable = HashSet<String>()
    /** Videos a frame of which was found unfilled (green), not to be put end to end as they are: by file. */
    private val broken = HashSet<String>()
    /** Videos made and read back right while open (their first frame and how many): their pictures may go. */
    private val checked = HashSet<Pair<Int, Int>>()
    /** The share of the last frame the system's reader gave that was the green of a frame left unfilled. */
    private var lastGreen = 0f

    private fun clipFrame(i: Int): Bitmap? = synchronized(clipLock) {
        val within = { c: io.github.artcraftandroid.procraft.core.TimeLapseClip -> i >= c.first.toInt() && i - c.first.toInt() < c.frames.toInt() }
        // the videos read again when one isn't there: another may have been made since
        val clip = (clips ?: core.clips().also { clips = it }).firstOrNull(within)
            ?: core.clips().also { clips = it }.firstOrNull(within) ?: return null
        // the device's decoder first: its frames read in order and each checked, never one left
        // unfilled (green) shown (the one before stays); the system's reader where it can't be used
        decoderFrame(clip, i - clip.first.toInt())?.let { return it.bitmap }
        val picture = systemFrame(clip, i) ?: return null
        // a frame the system's reader gives mostly the green of one left unfilled, where the one before
        // it read had little of it: not shown (a stroke of that very colour loses one frame at most)
        val green = greenShare(picture)
        val flash = green > Yuv.UNFILLED_MOST && lastGreen < green / 4
        lastGreen = green
        if (flash) {
            AppLog.w("HanaCreate", "a time-lapse video's frame $i reads back unfilled here: the one before shows in its place")
            broken += clip.path
            picture.recycle()
            return null
        }
        picture
    }

    /** What the device's decoder made of a frame: its picture, or none where it left it unfilled. */
    private class Decoded(val bitmap: Bitmap?)

    /**
     * Frame [at] of [clip] from the device's decoder (going on from the frame read before it); null
     * where the decoder can't be used here or gives no such frame, for the system's reader to try.
     */
    private fun decoderFrame(clip: io.github.artcraftandroid.procraft.core.TimeLapseClip, at: Int): Decoded? {
        if (clip.path in undecodable) return null
        val frames = decoding?.takeIf { it.first == clip.path }?.second ?: run {
            decoding?.second?.close()
            decoding = null
            VideoFrames.open(clip.path)?.also { decoding = clip.path to it }
        } ?: return null.also { undecodable += clip.path }
        return try {
            when (val f = frames.bitmap(at)) {
                is VideoFrames.Frame.Shown -> Decoded(f.bitmap)
                VideoFrames.Frame.Unfilled -> {
                    if (broken.add(clip.path)) AppLog.w("HanaCreate", "${File(clip.path).name}'s frame $at decodes unfilled here: the one before shows in its place")
                    Decoded(null)
                }
                VideoFrames.Frame.None -> null
            }
        } catch (e: Exception) {
            // a decoder that can't be used here: the system's reader from now on
            AppLog.w("HanaCreate", "${File(clip.path).name}'s frames can't be decoded here: the system's reader reads them", e)
            undecodable += clip.path
            decoding?.second?.close()
            decoding = null
            null
        }
    }

    /** Frame [i] (in [clip]) as the system's reader gives it, a few read together. */
    private fun systemFrame(clip: io.github.artcraftandroid.procraft.core.TimeLapseClip, i: Int): Bitmap? {
        // (a copy that can't be made, memory short with the canvas open: the run let go, the frame read alone)
        decoded?.let { (first, frames) -> frames.getOrNull(i - first)?.let { b -> b.copy(Bitmap.Config.ARGB_8888, false)?.let { return it } ?: forgetRun() } }
        val retriever = reading?.takeIf { it.first == clip.path }?.second ?: try {
            reading?.second?.release()
            reading = null
            android.media.MediaMetadataRetriever().also { r ->
                r.setDataSource(clip.path)
                reading = clip.path to r
            }
        } catch (e: Exception) {
            AppLog.e("HanaCreate", "a time-lapse's video can't be opened here: ${File(clip.path).name}, ${File(clip.path).length()} bytes", e)
            return null
        }
        val at = i - clip.first.toInt()
        // as many as fit in RUN_BYTES (a 4K video's frames are 33 MB each); before Android 9 a
        // retriever reads no frames by number
        val frames = if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.P) emptyList() else {
            val side = { key: Int -> retriever.extractMetadata(key)?.toLongOrNull() ?: 0L }
            val bytes = side(android.media.MediaMetadataRetriever.METADATA_KEY_VIDEO_WIDTH) * side(android.media.MediaMetadataRetriever.METADATA_KEY_VIDEO_HEIGHT) * 4
            val together = if (bytes > 0) (RUN_BYTES / bytes).toInt().coerceIn(1, CLIP_RUN) else CLIP_RUN
            runCatching { retriever.getFramesAtIndex(at, minOf(together, clip.frames.toInt() - at)) }
                .onFailure { AppLog.w("HanaCreate", "a time-lapse video's frames by number", it) }.getOrNull().orEmpty()
        }
        if (frames.isEmpty()) return alone(retriever, at, clip.frames.toInt())
        decoded?.second?.forEach { it.recycle() }
        decoded = i to frames
        return frames.first().copy(Bitmap.Config.ARGB_8888, false) ?: run {
            forgetRun()
            alone(retriever, at, clip.frames.toInt())
        }
    }

    /**
     * The share of [GREEN_GRID] x [GREEN_GRID] points across [picture] that are the green a frame left
     * unfilled shows (its YUV all zero: no red or blue, green about 135).
     */
    private fun greenShare(picture: Bitmap): Float {
        val (w, h) = picture.width to picture.height
        if (w <= 0 || h <= 0) return 0f
        var green = 0
        val row = IntArray(w)
        for (j in 0 until GREEN_GRID) {
            picture.getPixels(row, 0, w, 0, ((2 * j + 1) * h / (2 * GREEN_GRID)).coerceIn(0, h - 1), w, 1)
            for (i in 0 until GREEN_GRID) {
                val c = row[((2 * i + 1) * w / (2 * GREEN_GRID)).coerceIn(0, w - 1)]
                if (Color.red(c) < 24 && Color.blue(c) < 24 && Color.green(c) in 105..165) green++
            }
        }
        return green / (GREEN_GRID * GREEN_GRID).toFloat()
    }

    /** Frame [at] of a video of [frames] read by itself: by its number (from Android 9), else by its time. */
    private fun alone(retriever: android.media.MediaMetadataRetriever, at: Int, frames: Int): Bitmap? =
        (if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.P) null else {
            runCatching { retriever.getFrameAtIndex(at) }.onFailure { AppLog.w("HanaCreate", "a time-lapse video's frame by number", it) }.getOrNull()
        }) ?: frameAtTime(retriever, at, frames)

    /** The frames read together let go. */
    private fun forgetRun() {
        decoded?.second?.forEach { it.recycle() }
        decoded = null
    }

    /** Frame [at] of a video of [frames] by its time: not every device's retriever goes by frame number. */
    private fun frameAtTime(retriever: android.media.MediaMetadataRetriever, at: Int, frames: Int): Bitmap? = runCatching {
        val length = retriever.extractMetadata(android.media.MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull()?.times(1000)
            ?: (frames * 1_000_000L / FPS)
        retriever.getFrameAtTime(at * length / frames, android.media.MediaMetadataRetriever.OPTION_CLOSEST)
    }.onFailure { AppLog.w("HanaCreate", "a time-lapse video's frame by time", it) }.getOrNull()

    /** The videos read again next time (they changed), and the one being read let go. */
    private fun forgetClips() = synchronized(clipLock) {
        clips = null
        reading?.second?.release()
        reading = null
        decoding?.second?.close()
        decoding = null
        lastGreen = 0f
        decoded?.second?.forEach { it.recycle() }
        decoded = null
    }

    /** What it holds while reading its videos let go (the artwork is left). */
    fun close() = forgetClips()

    /**
     * The frames as an H.264 MP4 at [out]: all of them, or (with [seconds]) spread evenly over
     * that long. [going] hears how far it has come (0..1) before each frame, and stops it by saying
     * false. False if there are none or the encoder fails.
     */
    fun export(out: File, seconds: Int?, going: (Float) -> Boolean = { true }): Boolean {
        val started = android.os.SystemClock.elapsedRealtime()
        val total = count
        val picks = timeLapsePicks(total.toUInt(), seconds?.toUInt()).map { it.toInt() }
        val video = settings
        // what it was asked, and how it ended, in the log: a failure with the step it got to
        AppLog.i(
            "HanaCreate",
            "exporting the time-lapse: ${picks.size} of $total frames${seconds?.let { " ($it s)" }.orEmpty()}, " +
                "${video.side} at most, ${video.bitsPerPixel} bits a pixel, ${if (video.hevc) "HEVC" else "H.264"}",
        )
        failure = null
        var stopped = false
        val made = export(out, picks, total) { done -> going(done).also { if (!it) stopped = true } }
        val ms = android.os.SystemClock.elapsedRealtime() - started
        when {
            stopped -> AppLog.i("HanaCreate", "the time-lapse export stopped after $ms ms")
            made -> AppLog.i("HanaCreate", "the time-lapse exported in $ms ms: ${out.length()} bytes${unread.takeIf { it > 0 }?.let { ", $it frames unread" }.orEmpty()}")
            else -> AppLog.w("HanaCreate", "the time-lapse couldn't be exported (after $ms ms): ${failure ?: "the video wasn't made"}")
        }
        return made
    }

    private fun export(out: File, picks: List<Int>, total: Int, going: (Float) -> Boolean): Boolean {
        // all of it, as asked or too short to cut: the videos kept for packages, made where they
        // don't reach yet, put end to end, nothing read back from them
        if (picks.size == total) joined(out, going)?.let { return it }
        // (a device with no HEVC encoder makes it H.264, rather than nothing)
        return encode(out, picks, null, going = going, orH264 = true) != null
    }

    /** Why the last video wasn't made, for the log: the step it got to, the codec and size. */
    @Volatile
    private var failure: String? = null

    /**
     * Every frame at [out] as the package's videos joined: the frames in none yet made into one
     * first (as [forPackage] makes them), the videos then copied one after another, nothing made
     * again. False if [going] stopped it (the videos kept as they were); null if they can't be
     * joined, so it is made afresh.
     */
    private fun joined(out: File, going: (Float) -> Boolean): Boolean? {
        val total = count
        if (total <= 0) return null
        var plan = core.segmentPlan()
        while (plan != null) {
            val todo = plan.frames.size
            val before = total - todo
            var stopped = false
            val size = if (plan.width > 0u && plan.height > 0u) plan.width.toInt() to plan.height.toInt() else null
            val made = encode(File(plan.path), plan.frames.map { it.toInt() }, size, plan.hevc) { done ->
                going((before + done * todo) / total).also { stopped = !it }
            }
            if (stopped) {
                // not kept: the next one makes it again
                File(plan.path).delete()
                return false
            }
            if (made == null) AppLog.w("HanaCreate", "the time-lapse's video of frames ${plan.frames.firstOrNull()} to ${plan.frames.lastOrNull()} wasn't made (${failure ?: "?"}): planned again")
            plan = core.segmentMade(plan, (made?.first ?: 0).toUInt(), (made?.second ?: 0).toUInt(), made != null)
            if (made == null && plan == null) return null
        }
        // a video that couldn't be noted (or one gone): the frames out of them are made afresh
        if (!core.allInVideos()) return null.also { AppLog.i("HanaCreate", "the time-lapse's frames aren't all in its videos: made afresh") }
        dropClipFrames()
        // a video a frame of which read back unfilled here (made before videos were read back as
        // they are made) isn't copied as it is: made afresh, with the frame before in its place
        if (synchronized(clipLock) { broken.isNotEmpty() }) return null.also { AppLog.i("HanaCreate", "a time-lapse video reads back unfilled here: made afresh") }
        val parts = core.forPackage().segments.map(::File)
        if (parts.isNotEmpty() && VideoWriter.join(parts, out, FPS.toFloat())) return true
        AppLog.w("HanaCreate", "the time-lapse's ${parts.size} videos (${parts.sumOf { it.length() }} bytes) couldn't be put end to end: made afresh")
        return null
    }

    /**
     * The frames [picks] as a video at [out], HEVC or H.264 as [hevc] says, each fitted on white
     * into [size] (else into the first readable one's, within the settings' side, its sides whole
     * blocks of [ALIGN]); its size, or null if none can be read or the encoder fails. A frame that
     * can't be read shows the one before it again (counted in [unread]), so every frame keeps its
     * place. The video is read back, and made by the next encoder where its frames don't show what
     * they were given; one of frames in a row that reads back right is noted, its pictures free to
     * go. [going] as for [export]: the frames after it says false are left out.
     */
    private fun encode(
        out: File,
        picks: List<Int>,
        size: Pair<Int, Int>?,
        hevc: Boolean = settings.hevc,
        orH264: Boolean = false,
        going: (Float) -> Boolean = { true },
    ): Pair<Int, Int>? {
        unread = 0
        if (picks.isEmpty()) return null.also { failure = "no frames to make it of" }
        val video = settings
        var mime = if (hevc) android.media.MediaFormat.MIMETYPE_VIDEO_HEVC else android.media.MediaFormat.MIMETYPE_VIDEO_AVC
        val (w, h) = size ?: run {
            val first = picks.asSequence().take(FIRST_TRIES).firstNotNullOfOrNull { frame(it) }
                ?: return null.also { failure = "none of its first ${minOf(FIRST_TRIES, picks.size)} frames could be read (frame ${picks.first()}: ${core.frameFile(picks.first().toUInt()) != null} a picture, ${kept(picks.first())} kept)" }
            val fitted = VideoWriter.fit(first.width, first.height, video.side, mime, ALIGN)
                ?: if (hevc && orH264) {
                    AppLog.w("HanaCreate", "no HEVC encoder here takes the time-lapse's ${first.width}x${first.height} frames: H.264 instead")
                    mime = android.media.MediaFormat.MIMETYPE_VIDEO_AVC
                    VideoWriter.fit(first.width, first.height, video.side, mime, ALIGN)
                } else {
                    null
                }
            fitted.also {
                if (it == null) failure = "no $mime encoder here takes its ${first.width}x${first.height} frames, ${video.side} at most"
                first.recycle()
            }
        } ?: return null
        val frameBitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888).apply { eraseColor(Color.WHITE) }
        val paint = Paint(Paint.FILTER_BITMAP_FLAG)
        var missed = 0
        try {
            val made = VideoWriter.make(out, w, h, FPS.toFloat(), picks.size, mime, video.bitsPerPixel, check = true) { n, pixels ->
                if (!going(n / picks.size.toFloat())) return@make false
                val source = frame(picks[n])
                if (source == null) {
                    missed++
                } else {
                    fitOnWhite(frameBitmap, source, paint)
                    source.recycle()
                }
                frameBitmap.getPixels(pixels, 0, w, 0, 0, w, h)
                true
            }
            if (missed > 0) AppLog.w("HanaCreate", "$missed of ${picks.size} time-lapse frames couldn't be read: the one before each shows in its place")
            unread = missed
            val inRow = picks.last() - picks.first() == picks.size - 1
            if (made == VideoWriter.Made.CHECKED && missed == 0 && inRow) synchronized(clipLock) { checked += picks.first() to picks.size }
            if (made == VideoWriter.Made.NONE) failure = "the ${w}x$h $mime video of ${picks.size} frames wasn't made (the encoders' lines above say why)"
            return if (made != VideoWriter.Made.NONE) w to h else null
        } finally {
            frameBitmap.recycle()
        }
    }

    /**
     * The time-lapse as a package keeps it: its settings, and its frames made into videos (the
     * ones made before kept, so a share makes only what is new; the pictures they hold then gone,
     * as the artwork file keeps a time-lapse: only as video). Leaving the artwork does this too.
     */
    fun forPackage(): PackageTimeLapse {
        var plan = core.segmentPlan()
        while (plan != null) {
            val size = if (plan.width > 0u && plan.height > 0u) plan.width.toInt() to plan.height.toInt() else null
            val made = encode(File(plan.path), plan.frames.map { it.toInt() }, size, plan.hevc)
            plan = core.segmentMade(plan, (made?.first ?: 0).toUInt(), (made?.second ?: 0).toUInt(), made != null)
        }
        dropClipFrames()
        return core.forPackage()
    }

    /**
     * The pictures its videos hold too gone (the core checks each video's frames first), once
     * this device has read every frame of those videos back as its picture shows (a video made
     * and read back so while open isn't read again): where it can't, the pictures are all there
     * is to replay, and they stay.
     */
    private fun dropClipFrames() {
        forgetClips()
        val readable = core.clipsWithPictures().all { clip ->
            synchronized(clipLock) { clip.first.toInt() to clip.frames.toInt() in checked } || readsAsPictures(clip)
        }
        forgetClips()
        if (!readable) {
            AppLog.w("HanaCreate", "the time-lapse's videos can't be read here: their pictures stay")
            return
        }
        val gone = core.dropClipFrames()
        if (gone > 0u) android.util.Log.i("HanaCreate", "the time-lapse's videos hold $gone frames: their pictures went")
    }

    /**
     * Whether every frame of [clip] decodes here as its picture shows (none unfilled, none apart
     * from its picture fitted as [encode] fits it); where the device's decoder can't be used, its
     * first and last frames read by the system's reader, as before.
     */
    private fun readsAsPictures(clip: io.github.artcraftandroid.procraft.core.TimeLapseClip): Boolean {
        val first = clip.first.toInt()
        val frames = VideoFrames.open(clip.path)
            ?: return listOf(first, first + clip.frames.toInt() - 1).all { f -> clipFrame(f)?.also { it.recycle() } != null }
        return try {
            frames.use {
                if (it.count != clip.frames.toInt()) return false
                (0 until it.count).all { k ->
                    var size = 0 to 0
                    val look = it.read(k) { p, m ->
                        size = p.width to p.height
                        Yuv.cells(p, m).takeIf { Yuv.unfilledShare(p) <= Yuv.UNFILLED_MOST }
                    }
                    val picture = core.frameFile((first + k).toUInt())?.let { f -> pictureLook(f, size.first, size.second) }
                    val right = look != null && picture != null && !Yuv.apart(look, picture)
                    if (!right) AppLog.w("HanaCreate", "${File(clip.path).name}'s frame $k of ${it.count} doesn't read back as its picture here")
                    right
                }
            }
        } catch (e: Exception) {
            AppLog.w("HanaCreate", "reading a time-lapse video back", e)
            false
        }
    }

    /** The picture in [file] as a [width] x [height] frame of it shows it (fitted on white, as [encode] fits it): its [Yuv.cells]. */
    private fun pictureLook(file: String, width: Int, height: Int): ByteArray? {
        if (width <= 0 || height <= 0) return null
        val source = runCatching { BitmapFactory.decodeFile(file) }.getOrNull() ?: return null
        val frame = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        return try {
            fitOnWhite(frame, source, Paint(Paint.FILTER_BITMAP_FLAG))
            val argb = IntArray(width * height)
            frame.getPixels(argb, 0, width, 0, 0, width, height)
            Yuv.cells(argb, width, height)
        } finally {
            frame.recycle()
            source.recycle()
        }
    }

    /** [source] drawn into all of [frame] as a video's frame shows it: fitted, in the middle, on white. */
    private fun fitOnWhite(frame: Bitmap, source: Bitmap, paint: Paint) {
        val (w, h) = frame.width to frame.height
        Canvas(frame).apply {
            drawColor(Color.WHITE)
            val s = minOf(w / source.width.toFloat(), h / source.height.toFloat())
            val dw = (source.width * s).toInt()
            val dh = (source.height * s).toInt()
            drawBitmap(source, null, Rect((w - dw) / 2, (h - dh) / 2, (w + dw) / 2, (h + dh) / 2), paint)
        }
    }

    /**
     * Takes a package's time-lapse [t]: as the package keeps it, its videos as they are (nothing
     * decoded); else (a video whose frames can't be counted) its settings, and its videos' frames
     * after these, the videos themselves kept to go back into a package as they are when every
     * frame of them came in and there were no frames before.
     */
    fun take(t: PackageTimeLapse) {
        if (core.takeClips(t)) return
        core.takeBegin(t)
        for (path in t.segments) {
            var frames = 0
            val retriever = android.media.MediaMetadataRetriever()
            try {
                retriever.setDataSource(path)
                if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.P) {
                    frames = keepFramesByTime(retriever)
                } else {
                    frames = retriever.extractMetadata(android.media.MediaMetadataRetriever.METADATA_KEY_VIDEO_FRAME_COUNT)?.toIntOrNull() ?: 0
                    // a few at a time: one seek for each run, not each frame
                    var i = 0
                    while (i < frames) {
                        val run = minOf(DECODE_RUN, frames - i)
                        val pictures = runCatching { retriever.getFramesAtIndex(i, run) }.getOrNull() ?: break
                        pictures.forEach { keepFrame(it); it.recycle() }
                        i += run
                    }
                }
            } catch (e: Exception) {
                AppLog.e("HanaCreate", "a time-lapse's video", e)
                frames = 0
            } finally {
                retriever.release()
            }
            core.takeVideo(path, frames.toUInt())
        }
        core.takeEnd()
    }

    /**
     * Before Android 9, whose retriever neither counts a video's frames nor reads them by number: the
     * frames of a package's video read by their times, as long as it lasts at the time-lapse's
     * rate, each kept as the next frame; how many.
     */
    private fun keepFramesByTime(retriever: android.media.MediaMetadataRetriever): Int {
        val length = retriever.extractMetadata(android.media.MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull() ?: return 0
        val count = (length * FPS + 500) / 1000
        var kept = 0
        while (kept < count) {
            val picture = runCatching {
                retriever.getFrameAtTime(kept * 1_000_000L / FPS, android.media.MediaMetadataRetriever.OPTION_CLOSEST)
            }.getOrNull() ?: break
            keepFrame(picture)
            picture.recycle()
            kept++
        }
        return kept
    }

    /** A package video's frame [picture] kept as the next frame. */
    private fun keepFrame(picture: Bitmap) {
        val argb = if (picture.config == Bitmap.Config.ARGB_8888) picture else picture.copy(Bitmap.Config.ARGB_8888, false)
        val rgba = ByteBuffer.allocate(argb.width * argb.height * 4).also { argb.copyPixelsToBuffer(it) }.array()
        core.keepFrame(rgba, argb.width.toUInt(), argb.height.toUInt())
        if (argb !== picture) argb.recycle()
    }

    companion object {
        val FPS = timeLapseFps().toInt()
        /** Frames decoded together from a package's video, and from one the frames are in. */
        private const val DECODE_RUN = 8
        private const val CLIP_RUN = 15

        /** A video's sides are made whole blocks of this, as every codec lays them out (none padded or cropped). */
        private const val ALIGN = 16

        /** Points across a frame the system's reader gives, looked at for the green of one left unfilled. */
        private const val GREEN_GRID = 48

        /** The most a run of frames read together from a video may take, bytes. */
        private const val RUN_BYTES = 64L shl 20

        /** Frames tried for the first readable one (each can be a slow decode). */
        private const val FIRST_TRIES = 120
    }
}
