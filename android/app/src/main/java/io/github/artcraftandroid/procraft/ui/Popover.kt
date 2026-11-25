package io.github.artcraftandroid.procraft.ui

import android.graphics.BlurMaskFilter
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.graphics.ClipOp
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Paint
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionOnScreen
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** Where frosted panels are told about: on the canvas screen only, which has a canvas behind them. */
val LocalBackdrops = staticCompositionLocalOf<Backdrops?> { null }

/**
 * The rounded bodies of the frosted panels and bars shown now, and the thin glass of the side bar's
 * tracks (window pixels), sent on whenever they change.
 */
class Backdrops(private val send: (List<Float>, List<Float>) -> Unit) {
    /**
     * Where the canvas's corner is on the screen (pixels), so a place on the screen is made one on
     * the canvas: see [canvasCorner].
     */
    var canvasOnScreen: () -> Offset = { Offset.Zero }

    /** Where [onScreen] (pixels) is on the canvas. */
    fun onCanvas(onScreen: Offset): Offset = onScreen - canvasOnScreen()

    private val regions = LinkedHashMap<Any, FloatArray>()
    private val windows = LinkedHashMap<Any, FloatArray>()

    private fun sendAll() = send(regions.values.flatMap { it.asList() }, windows.values.flatMap { it.asList() })

    fun put(key: Any, region: FloatArray, window: Boolean = false) {
        val into = if (window) windows else regions
        if (into[key]?.contentEquals(region) == true) return
        into[key] = region
        sendAll()
    }

    fun remove(key: Any) {
        if (regions.remove(key) == null && windows.remove(key) == null) return
        sendAll()
    }
}

/**
 * Makes the node it is put on frosted glass over the canvas while it is shown: what is behind its
 * rounded box ([corner]) is blurred. Works from popups too (it goes by screen position).
 */
@Composable
fun rememberFrost(corner: Dp): Modifier = rememberFrost(corner, BarGlass())

/**
 * How a bar's glass differs from a panel's: it shows as much as [shown], [moved] (window pixels) as
 * the bar slides out of its place, its box goes [pastLeft], [pastRight] and [pastBottom] further (to
 * take the corners of a bar's or a sheet's screen edge off the screen), and as a [window] what is
 * behind only softens.
 */
data class BarGlass(
    val shown: Float = 1f,
    val moved: Offset = Offset.Zero,
    val pastLeft: Dp = 0.dp,
    val pastRight: Dp = 0.dp,
    val window: Boolean = false,
    val pastBottom: Dp = 0.dp,
)

/** [rememberFrost] for a bar, as [glass] says. */
@Composable
fun rememberFrost(corner: Dp, glass: BarGlass): Modifier {
    val backdrops = LocalBackdrops.current ?: return Modifier
    val key = remember { Any() }
    val density = LocalDensity.current
    // the blur grows and fades with the panel
    val look = LocalPanelLook.current
    var box by remember { mutableStateOf<FloatArray?>(null) }
    DisposableEffect(backdrops) { onDispose { backdrops.remove(key) } }
    // read here, not in the effect: a bar that never changes must still be told where it landed
    val placed = box
    SideEffect {
        placed?.let { (x0, y0, x1, y1, corner) ->
            val (px, py, k) = Triple(look.pivot.x, look.pivot.y, look.scale)
            val (dx, dy) = glass.moved
            val region = floatArrayOf(
                px + (x0 + dx - px) * k, py + (y0 + dy - py) * k, px + (x1 + dx - px) * k, py + (y1 + dy - py) * k,
                corner * k, look.alpha * glass.shown,
            )
            backdrops.put(key, region, glass.window)
        }
    }
    return Modifier.onGloballyPositioned { c ->
        // by the screen, so that it works from popups too, then on the canvas
        val at = backdrops.onCanvas(c.positionOnScreen())
        val (left, right) = with(density) { glass.pastLeft.toPx() to glass.pastRight.toPx() }
        val below = with(density) { glass.pastBottom.toPx() }
        box = frostBox(at, c.size.width, c.size.height, left, right, below, with(density) { corner.toPx() })
    }.drawBehind {
        val progress = glass.shown.coerceIn(0f, 1f)
        val start = size.width * (progress * Motion.GLASS_SWEEP_TRAVEL - 0.8f)
        val sheen = Brush.linearGradient(
            colors = listOf(Color.Transparent, Pc.MenuInk.copy(alpha = Motion.GLASS_SWEEP_ALPHA), Color.Transparent),
            start = Offset(start, 0f),
            end = Offset(start + size.width * 0.8f, size.height),
        )
        drawRoundRect(sheen, cornerRadius = CornerRadius(corner.toPx()))
    }
}

/**
 * Where the canvas's corner is on the screen (pixels): the corner of the window's content
 * ([contentOnScreen]), moved in by what the interface keeps [clear] of. The content's, not the
 * window's: in a window of its own or a split screen the window's frame holds a caption bar above
 * the content, so the frame's corner is that much higher than the canvas's.
 */
internal fun canvasCorner(contentOnScreen: Offset, clear: Clearance): Offset = contentOnScreen + clear.corner

/**
 * The frosted box (x0, y0, x1, y1 and its corners' radius, canvas pixels) of a node [width] by
 * [height] at [onCanvas], [left], [right] and [below] further out than it.
 */
internal fun frostBox(onCanvas: Offset, width: Int, height: Int, left: Float, right: Float, below: Float, corner: Float): FloatArray =
    floatArrayOf(onCanvas.x - left, onCanvas.y, onCanvas.x + width + right, onCanvas.y + height + below, corner)


/** The side of a popover its arrow sticks out of, pointing at its button; or none. */
enum class ArrowSide { TOP, BOTTOM, LEFT, RIGHT, NONE }

/**
 * Where the panel opening now hangs from a button Edit layout moved: the side its arrow is on, and how
 * far along that side its tip is (the panel's own dp). The panel's own popover takes it in place of
 * an arrow up from under the top bar; nothing that opens inside it does.
 */
class Hang(val side: ArrowSide, val arrow: Dp)

/** The hang of the panel opening now, while Edit layout moved its button. */
val LocalHang = staticCompositionLocalOf<Hang?> { null }

/**
 * A panel floating in a window of its own (every panel may, while a plugin lets it): how wide and tall the window lays it
 * out (its own dp), and what its strip and corner do. The panel's own popover draws as that window, with no arrow and a
 * strip over it; nothing that opens inside it does.
 */
class FloatWindow(val width: Dp, val height: Dp, val frame: FloatingFrame, val sizable: Boolean)

/** The window the panel showing now floats in, if it does. */
val LocalFloat = staticCompositionLocalOf<FloatWindow?> { null }

/**
 * A panel hanging from the top bar, with an arrow pointing up at its button: [arrowX] is the tip's
 * distance from the panel's left edge (from its top, for an arrow on the [side]). [popoverPlace]
 * finds where it goes. Without a [fill] of its own it is frosted over the canvas: translucent,
 * with what is behind it blurred. [width] is the body's: a side arrow adds to it. Hanging from a
 * button Edit layout moved ([LocalHang]), an arrow that would point up points at it instead.
 */
@Composable
fun Popover(
    width: Dp,
    arrowX: Dp,
    modifier: Modifier = Modifier,
    fill: Color? = null,
    corner: Dp = Pc.PopoverCorner,
    side: ArrowSide = ArrowSide.TOP,
    content: @Composable ColumnScope.() -> Unit,
) {
    val hang = LocalHang.current?.takeIf { side == ArrowSide.TOP }
    val float = LocalFloat.current?.takeIf { side == ArrowSide.TOP }
    androidx.compose.runtime.CompositionLocalProvider(LocalHang provides null, LocalFloat provides null) {
        if (float != null) {
            // floating: as big as its window, its strip over it and the room for its corner under it
            PopoverBody(float.width, 0.dp, modifier.height(float.height), fill, corner, ArrowSide.NONE) {
                FloatingTop(float.frame)
                androidx.compose.foundation.layout.Box(Modifier.weight(1f).fillMaxWidth()) {
                    Column(Modifier.fillMaxSize(), content = content)
                }
                if (float.sizable) FloatingBottom(float.frame)
            }
        } else {
            PopoverBody(width, hang?.arrow ?: arrowX, modifier, fill, corner, hang?.side ?: side, content)
        }
    }
}

@Composable
private fun PopoverBody(
    width: Dp,
    arrowX: Dp,
    modifier: Modifier,
    fill: Color?,
    corner: Dp,
    side: ArrowSide,
    content: @Composable ColumnScope.() -> Unit,
) {
    val backdrops = LocalBackdrops.current.takeIf { fill == null && Pc.PopoverFrost.alpha < 1f }
    val paint = fill ?: if (backdrops != null) Pc.PopoverFrost else Pc.PopoverFill
    val key = remember { Any() }
    val density = LocalDensity.current
    // the blur grows and fades with the panel: its body as drawn (already grown), and how much
    // it shows
    val look = LocalPanelLook.current
    var body by remember { mutableStateOf<FloatArray?>(null) }
    if (backdrops != null) {
        DisposableEffect(backdrops) { onDispose { backdrops.remove(key) } }
        // read here, so its being placed (or moved) comes back through here
        val placed = body
        SideEffect { placed?.let { backdrops.put(key, it + look.alpha) } }
    }
    val frost = if (backdrops == null) {
        Modifier
    } else {
        Modifier.onGloballyPositioned { c ->
            // by the screen, as a bar's (see rememberFrost)
            val at = backdrops.onCanvas(c.positionOnScreen())
            val k = look.scale
            val (arrowH, radius) = with(density) { Pc.ArrowHeight.toPx() to corner.toPx() }
            val (l, t, r) = Triple(if (side == ArrowSide.LEFT) arrowH else 0f, if (side == ArrowSide.TOP) arrowH else 0f, if (side == ArrowSide.RIGHT) arrowH else 0f)
            val b = if (side == ArrowSide.BOTTOM) arrowH else 0f
            body = floatArrayOf(at.x + l * k, at.y + t * k, at.x + (c.size.width - r) * k, at.y + (c.size.height - b) * k, radius * k)
        }
    }
    val sideways = side == ArrowSide.LEFT || side == ArrowSide.RIGHT
    Box(
        modifier.width(if (sideways) width + Pc.ArrowHeight else width).then(frost).drawBehind {
            val arrowH = Pc.ArrowHeight.toPx()
            val at = arrowX.toPx()
            val half = arrowH
            val tip = 2.dp.toPx()
            val radius = CornerRadius(corner.toPx())
            // a corner the arrow comes near is rounded less, so that the arrow rises from a
            // straight edge rather than hanging over the curve (at a small interface size, or a
            // button near the screen's edge)
            val near = { from: Float -> CornerRadius((from - half - tip).coerceIn(0f, corner.toPx())) }
            val outline = Path().apply {
                when (side) {
                    ArrowSide.TOP -> {
                        addRoundRect(RoundRect(0f, arrowH, size.width, size.height, near(at), near(size.width - at), radius, radius))
                        moveTo(at - half - tip, arrowH + 0.5f)
                        lineTo(at - tip, tip)
                        quadraticTo(at, 0f, at + tip, tip)
                        lineTo(at + half + tip, arrowH + 0.5f)
                        close()
                    }
                    ArrowSide.BOTTOM -> {
                        val h = size.height
                        addRoundRect(RoundRect(0f, 0f, size.width, h - arrowH, radius, radius, near(size.width - at), near(at)))
                        moveTo(at - half - tip, h - arrowH - 0.5f)
                        lineTo(at - tip, h - tip)
                        quadraticTo(at, h, at + tip, h - tip)
                        lineTo(at + half + tip, h - arrowH - 0.5f)
                        close()
                    }
                    ArrowSide.LEFT -> {
                        addRoundRect(RoundRect(arrowH, 0f, size.width, size.height, near(at), radius, radius, near(size.height - at)))
                        moveTo(arrowH + 0.5f, at - half - tip)
                        lineTo(tip, at - tip)
                        quadraticTo(0f, at, tip, at + tip)
                        lineTo(arrowH + 0.5f, at + half + tip)
                        close()
                    }
                    ArrowSide.RIGHT -> {
                        val w = size.width
                        addRoundRect(RoundRect(0f, 0f, w - arrowH, size.height, radius, near(at), near(size.height - at), radius))
                        moveTo(w - arrowH - 0.5f, at - half - tip)
                        lineTo(w - tip, at - tip)
                        quadraticTo(w, at, w - tip, at + tip)
                        lineTo(w - arrowH - 0.5f, at + half + tip)
                        close()
                    }
                    ArrowSide.NONE -> addRoundRect(RoundRect(0f, 0f, size.width, size.height, radius))
                }
            }
            drawIntoCanvas { canvas ->
                val shadow = Paint().apply {
                    color = Pc.PopoverShadow
                    asFrameworkPaint().maskFilter = BlurMaskFilter(Pc.PopoverShadowRadius.toPx(), BlurMaskFilter.Blur.NORMAL)
                }
                // only around the panel: none of it shows through
                canvas.save()
                canvas.clipPath(outline, ClipOp.Difference)
                canvas.drawPath(outline, shadow)
                canvas.restore()
            }
            drawPath(outline, paint)
            if (backdrops != null) {
                val start = size.width * (look.alpha.coerceIn(0f, 1f) * Motion.GLASS_SWEEP_TRAVEL - 0.8f)
                val sheen = Brush.linearGradient(
                    colors = listOf(Color.Transparent, Pc.MenuInk.copy(alpha = Motion.GLASS_SWEEP_ALPHA), Color.Transparent),
                    start = Offset(start, 0f),
                    end = Offset(start + size.width * 0.8f, size.height),
                )
                drawPath(outline, sheen)
            }
        },
    ) {
        val inset = when (side) {
            ArrowSide.TOP -> Modifier.padding(top = Pc.ArrowHeight)
            ArrowSide.BOTTOM -> Modifier.padding(bottom = Pc.ArrowHeight)
            ArrowSide.LEFT -> Modifier.padding(start = Pc.ArrowHeight)
            ArrowSide.RIGHT -> Modifier.padding(end = Pc.ArrowHeight)
            ArrowSide.NONE -> Modifier
        }
        Column(inset.clip(RoundedCornerShape(corner)), content = content)
    }
}

/** Where a popover [width] wide goes on a screen [screenWidth] wide: its left edge and arrow tip. */
data class PopoverPlace(val left: Dp, val arrowX: Dp)

/**
 * A popover is centred on its button (the tip sits [Pc.ArrowShift] right of the button's centre)
 * and kept [Pc.PopoverGap] inside the screen, and at least [minLeft] from its left edge (clear of
 * the side bar). In a window too narrow for all that (split screen) it goes in the window's middle
 * and over the side bar: what runs off the edge can't be reached.
 */
fun popoverPlace(width: Dp, buttonCentreX: Dp, screenWidth: Dp, minLeft: Dp = Pc.PopoverGap, minRight: Dp = Pc.PopoverGap): PopoverPlace {
    val tip = buttonCentreX + Pc.ArrowShift
    val furthest = screenWidth - minRight - width
    val left = if (furthest >= minLeft) (tip - width / 2).coerceIn(minLeft, furthest) else ((screenWidth - width) / 2).coerceAtLeast(0.dp)
    return PopoverPlace(left, tip - left)
}
