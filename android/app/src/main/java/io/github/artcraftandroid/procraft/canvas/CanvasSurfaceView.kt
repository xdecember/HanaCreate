package io.github.artcraftandroid.procraft.canvas

import android.annotation.SuppressLint
import android.content.Context
import android.view.MotionEvent
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.compose.ui.geometry.Offset
import androidx.input.motionprediction.MotionEventPredictor
import io.github.artcraftandroid.procraft.AppLog
import io.github.artcraftandroid.procraft.core.CanvasSession
import io.github.artcraftandroid.procraft.core.MobileException
import io.github.artcraftandroid.procraft.core.PenPreferred
import io.github.artcraftandroid.procraft.core.PenSample
import io.github.artcraftandroid.procraft.core.CompanionMode
import io.github.artcraftandroid.procraft.core.PointerKind
import io.github.artcraftandroid.procraft.core.TouchCommand
import io.github.artcraftandroid.procraft.core.TouchMove
import io.github.artcraftandroid.procraft.core.TransformHandle
import kotlin.math.atan2
import io.github.artcraftandroid.procraft.core.Adjustment

/**
 * The canvas: a surface the Rust renderer draws into, and the touch input that drives it.
 *
 * One pen or finger paints. A second finger turns a finger stroke into a gesture (the stroke is
 * dropped): two fingers move, zoom and turn the canvas; a quick tap with two fingers undoes, three
 * redo, four hide or show the controls. Fingers are ignored while the pen draws.
 */
@SuppressLint("ViewConstructor")
class CanvasSurfaceView(context: Context, private val vm: CanvasViewModel) : SurfaceView(context), SurfaceHolder.Callback {
    private var session: CanvasSession? = null
    private var attached = false

    // the window as last given to the session, and the display rotation it was in
    private var windowWidth = 0
    private var windowHeight = 0
    private var windowRotation = Surface.ROTATION_0

    /** Predicts where the pen is going, so the stroke can be drawn ahead of the latest input. */
    private val predictor = MotionEventPredictor.newInstance(this)

    /** The pen is hovering over the canvas. */
    private var penHovering = false

    /** How the pen hovering was last held (its lean's way; Android tells no barrel), for its cursor. */
    private var hoverPen: io.github.artcraftandroid.procraft.core.CursorPen? = null

    /** The pen's side buttons as its events last said. */
    private val penButtons = PenButtons()

    /** Where the pen last was over the canvas: a side button that comes as a key says nothing of it. */
    private var penAt: Offset? = null

    private var strokePointer = -1

    private val pickSide get() = (CanvasViewModel.PICK_WINDOW_DP * vm.density).toInt() or 1

    init {
        holder.addCallback(this)
    }

    override fun surfaceCreated(holder: SurfaceHolder) = Unit

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        val s = vm.session ?: return
        val rotation = display?.rotation ?: Surface.ROTATION_0
        if (attached && session === s) {
            if (rotation != windowRotation) followTurn(s, windowRotation, rotation)
            s.resizeWindow(width.toUInt(), height.toUInt())
            vm.refreshView()
            remember(width, height, rotation)
            return
        }
        val id = NativeWindows.acquire(holder.surface)
        if (id == 0L) {
            AppLog.e(TAG, "no native window for the surface")
            return
        }
        try {
            s.attachWindow(id.toULong(), width.toUInt(), height.toUInt())
            s.setDisplayScale(resources.displayMetrics.density)
            // the display turned while there was no window (the app away, the upright lock screen):
            // the view follows now, as it would have then, so the canvas stays put on the glass
            if (session === s && windowWidth > 0 && rotation != windowRotation) followTurn(s, windowRotation, rotation)
            session = s
            attached = true
            vm.refreshView()
            remember(width, height, rotation)
        } catch (e: MobileException) {
            NativeWindows.release(id)
            AppLog.e(TAG, "attaching the window", e)
        }
    }

    private fun remember(width: Int, height: Int, rotation: Int) {
        vm.session?.let { placeGrid(it, width, height, rotation) }
        // frames no more often than the screen can show them: its fastest refresh at this size
        display?.let { d ->
            val size = d.mode
            val fastest = d.supportedModes.filter { it.physicalWidth == size.physicalWidth && it.physicalHeight == size.physicalHeight }.maxOfOrNull { it.refreshRate }
            vm.session?.setRefreshRate(fastest ?: d.refreshRate)
        }
        vm.screenCentre = Offset(width / 2f, height / 2f)
        windowWidth = width
        windowHeight = height
        vm.windowSize = Offset(width.toFloat(), height.toFloat())
        vm.screenDensity = resources.displayMetrics.density
        windowRotation = rotation
    }

    /**
     * The surround's grid is fixed to the glass as the canvas is: its 1 pt lines lie every 16 pt
     * from the panel's own top-left corner in its natural orientation, so in the window a line's
     * middle is half a point in from whichever corner that is. The canvas moved in from the window's
     * corner (the interface keeping clear of a camera) has it that much further up and left.
     */
    private fun placeGrid(s: CanvasSession, width: Int, height: Int, rotation: Int) {
        val half = 0.5f * resources.displayMetrics.density
        val at = IntArray(2).also(::getLocationInWindow)
        val moved = at[0] != 0 || at[1] != 0
        val (w, h) = if (moved && rootView.width > 0 && rootView.height > 0) rootView.width to rootView.height else width to height
        val (nw, nh) = if (rotation % 2 == 0) w.toFloat() to h.toFloat() else h.toFloat() to w.toFloat()
        val (wx, wy) = when (rotation) {
            Surface.ROTATION_90 -> half to nw - half
            Surface.ROTATION_180 -> nw - half to nh - half
            Surface.ROTATION_270 -> nh - half to half
            else -> half to half
        }
        val (x, y) = wx - at[0] to wy - at[1]
        s.setGridOrigin(x, y)
        vm.gridOrigin = Offset(x, y)
    }

    /**
     * The display turned from rotation [from] to [to]: the view turns the other way, so the canvas
     * stays where it was on the glass while the bars turn upright.
     */
    private fun followTurn(s: CanvasSession, from: Int, to: Int) {
        // the panel's own size, in its natural orientation
        val (nw, nh) = if (from % 2 == 0) windowWidth.toFloat() to windowHeight.toFloat() else windowHeight.toFloat() to windowWidth.toFloat()
        fun toPanel(r: Int, x: Float, y: Float) = when (r) {
            Surface.ROTATION_90 -> nw - y to x
            Surface.ROTATION_180 -> nw - x to nh - y
            Surface.ROTATION_270 -> y to nh - x
            else -> x to y
        }
        fun fromPanel(r: Int, x: Float, y: Float) = when (r) {
            Surface.ROTATION_90 -> y to nw - x
            Surface.ROTATION_180 -> nw - x to nh - y
            Surface.ROTATION_270 -> nh - y to x
            else -> x to y
        }
        fun map(x: Float, y: Float) = toPanel(from, x, y).let { (px, py) -> fromPanel(to, px, py) }
        val (ox, oy) = map(0f, 0f)
        val (ux, uy) = map(1f, 0f)
        // (told apart from a turn of the fingers: the artwork's gallery turn counts only those)
        s.followScreenTurn(ox, oy, atan2(uy - oy, ux - ox))
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        // returns once the render thread has stopped drawing into the surface
        session?.detachWindow()
        attached = false
    }

    // every touch is the shared touches' (crates/core touch.rs): painting, picking, the snapped shape,
    // the gestures and every tool's touch, the same in every app

    /**
     * Palms (as the screen names them, or contacts too big for a fingertip on a screen that says how
     * big a touch is): the shared touches never see them, and one that becomes one is taken back.
     */
    private val palms = mutableSetOf<Int>()

    /** When the last pointer landed on the canvas (event time, ms). */
    private var landedAt = 0L

    /** Whether pointer [i] of [e] is a palm (see [palmContact]). */
    private fun isPalm(e: MotionEvent, i: Int): Boolean =
        palmContact(e.getToolType(i), kindOf(e, i), e.getTouchMajor(i), resources.displayMetrics.xdpi)

    /** Hands [e] to the shared touches and does what they ask. */
    private fun route(e: MotionEvent, s: CanvasSession) {
        val out = when (e.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
                val i = e.actionIndex
                if (e.actionMasked == MotionEvent.ACTION_DOWN) {
                    strokePointer = e.getPointerId(i)
                    palms.clear()
                }
                landedAt = e.eventTime
                tellPointer(e, i, "down")
                if (isPalm(e, i)) {
                    palms += e.getPointerId(i)
                    emptyList()
                } else {
                    val kind = kindOf(e, i)
                    // a pen's stroke goes by its lean only if its device says how it leans
                    if (kind == PointerKind.STYLUS || kind == PointerKind.STYLUS_ERASER) {
                        s.setPenTiltKnown(tiltKnown(e))
                        penStrokes.began(e.deviceId)
                        summedPointer = e.getPointerId(i)
                        sumPressures(e, i)
                    }
                    s.touchDown(e.getPointerId(i).toULong(), kind, samples(e, i))
                }
            }
            MotionEvent.ACTION_MOVE -> {
                if (penStrokes.summing) e.findPointerIndex(summedPointer).takeIf { it >= 0 }?.let { sumPressures(e, it) }
                val grown =(0 until e.pointerCount).filter { i -> e.getPointerId(i) !in palms && isPalm(e, i) }
                val back = grown.flatMap { i -> palms += e.getPointerId(i); s.touchCancelOne(e.getPointerId(i).toULong()) }
                back + s.touchMoves((0 until e.pointerCount).filter { i -> e.getPointerId(i) !in palms }.map { i -> TouchMove(e.getPointerId(i).toULong(), samples(e, i)) })
            }
            MotionEvent.ACTION_POINTER_UP, MotionEvent.ACTION_UP -> {
                val i = e.actionIndex
                if (penStrokes.summing && e.getPointerId(i) == summedPointer) {
                    sumPressures(e, i)
                    penStrokes.lifted()?.let { AppLog.i(TAG, it) }
                }
                when {
                    palms.remove(e.getPointerId(i)) -> emptyList()
                    // the system took the pointer back (a palm): what it did is undone, not ended
                    e.flags and MotionEvent.FLAG_CANCELED != 0 -> s.touchCancelOne(e.getPointerId(i).toULong())
                    else -> s.touchUp(e.getPointerId(i).toULong(), samples(e, i))
                }
            }
            MotionEvent.ACTION_CANCEL -> {
                // the tablet taking three fingers for its own gesture: said once, with what to do
                val fingers = (0 until e.pointerCount).count { kindOf(e, it) == PointerKind.FINGER }
                if (systemTookFingers(fingers, e.eventTime - landedAt, hasWindowFocus())) vm.systemTookFingers()
                penStrokes.lifted()?.let { AppLog.i(TAG, "$it (called off by the system)") }
                palms.clear()
                s.touchCancel()
            }
            else -> emptyList()
        }
        if (e.actionMasked == MotionEvent.ACTION_MOVE) predict(s)
        obey(out)
        scheduleTouchTick(s)
    }

    /** Does what the shared touches asked. */
    private fun obey(out: List<TouchCommand>) {
        for (c in out) when (c) {
            TouchCommand.Undo -> vm.undo()
            TouchCommand.Redo -> vm.redo()
            // a pen button set to a QuickMenu action in Pen buttons
            is TouchCommand.Run -> vm.runQuickAction(c.action, Offset(c.x, c.y))
            // one set to a plugin's action, run where the pen is
            is TouchCommand.RunPlugin -> vm.runPluginAction(c.key, Offset(c.x, c.y))
            TouchCommand.ClearLayer -> vm.clearCurrentLayer()
            TouchCommand.ToggleBars -> vm.toggleBars()
            TouchCommand.Fit -> vm.settleToFit()
            TouchCommand.CopyPaste -> vm.copyPasteOpen = true
            is TouchCommand.QuickMenu -> vm.toggleQuickMenu(Offset(c.x, c.y))
            is TouchCommand.LayerSelect -> vm.layerSelectAt(Offset(c.x, c.y))
            TouchCommand.QuickShapeLast -> vm.quickShapeLast()
            is TouchCommand.Pick -> vm.pickAt(c.x, c.y, pickSide)
            TouchCommand.PickDone -> vm.finishPick()
            TouchCommand.PickCancel -> vm.cancelPick()
            TouchCommand.ShapeSnapped -> vm.quickShapeSnapped()
            TouchCommand.ShapeChanged -> vm.quickShapeChanged()
            TouchCommand.ShapeEnded -> vm.quickShapeEnded()
            TouchCommand.StrokeEnded -> vm.strokeEnded()
            TouchCommand.ViewMoved -> vm.refreshView()
            TouchCommand.ViewSettles -> vm.followView()
            is TouchCommand.Cursor -> {
                vm.showCursor(Offset(c.x, c.y))
                // cloning, the source follows the brush
                if (vm.adjustment == Adjustment.CLONE) vm.followClonePuck()
            }
            TouchCommand.CursorGone -> {
                vm.showCursor(null)
                vm.transformHeld = null
            }
            // a pen touching at no pressure before it presses (some begin their touches above the glass; one
            // not known to give a pressure waits a moment for one, then paints at full pressure): as it hovering there
            is TouchCommand.PenHovers -> hovered(Offset(c.x, c.y))
            TouchCommand.TextLayer -> vm.askRasterize = true
            TouchCommand.BrushChanged -> vm.brushTouched()
            // Recolor's reticle dragged or tapped elsewhere (its fills on the view model's fill thread)
            is TouchCommand.RecolorMoved -> vm.recolorMoveTo(Offset(c.x, c.y))
            is TouchCommand.RecolorTapped -> vm.recolorTap(Offset(c.x, c.y))
            // Continue filling: the touch fills where it landed, its threshold slid redoes the fill, and let go (or
            // taken back by a second finger) it ends, each on the view model's fill thread; and the fill let go
            is TouchCommand.PourAt -> vm.tapFill(Offset(c.x, c.y), c.fill)
            is TouchCommand.FillThreshold -> vm.pourThreshold(c.threshold)
            is TouchCommand.PourEnd -> vm.tapFillEnd(c.keep)
            TouchCommand.Filled -> vm.filled()
            // a transform's touch: its popovers close; the box as it now is; the node held, and let go
            // (a rotate or box-turning node springs back); a node tapped types its numbers beside it
            TouchCommand.TransformTouched -> vm.transformPopup = TransformPopup.NONE
            is TouchCommand.TransformMoved -> vm.transformMoved(c.transform)
            is TouchCommand.TransformHeld -> vm.transformHeld = c.handle to Offset(c.x, c.y)
            is TouchCommand.TransformLetGo -> {
                vm.transformHeld = null
                if (c.handle == TransformHandle.Rotate || c.handle == TransformHandle.TurnBox) vm.transformLetGo = c.handle to Offset(c.x, c.y)
            }
            is TouchCommand.TransformNumeric -> vm.transformNumeric = CanvasViewModel.TransformNumeric(c.angle, Offset(c.x, c.y), c.right)
            // the selection tool's touch: a shape dragged out, a colour selection's threshold slid, the
            // freehand path drawn, a selection made
            is TouchCommand.SelectionDrag -> vm.shapeDrag = Offset(c.x0, c.y0) to Offset(c.x1, c.y1)
            is TouchCommand.SelectionThreshold -> vm.colourSlid(Offset(c.x, c.y), c.threshold, c.done)
            TouchCommand.FreehandChanged -> vm.freehandChanged()
            TouchCommand.SelectionChanged -> vm.selectionTouched()
            // an adjustment's amount, liquify, the clone source and the like changed; a tap opens its menu
            TouchCommand.AdjustChanged -> vm.adjustTouched()
            is TouchCommand.AdjustMenu -> if (!closedAdjustMenu) vm.adjustMenuAt = Offset(c.x, c.y)
            // what a pen's own settings ask its double-tap or squeeze to show
            TouchCommand.ShowColours -> vm.toggle(Panel.COLOR)
            TouchCommand.ShowBrushes -> vm.toggle(Panel.BRUSHES)
            // a touch on the canvas with a panel open: dragged, it fades the panel; tapped, it only closes it
            TouchCommand.PanelFades -> {
                vm.swipedBrush = null
                vm.fadePanels()
            }
            TouchCommand.PanelTapped -> vm.tapOutsidePanel()
            // the guide editor: a touch came down (symmetry's options close), dragged a handle or
            // added a point; a handle tapped shows its menu
            TouchCommand.GuideTouched -> vm.guideTouched(touched = true)
            TouchCommand.GuideChanged -> vm.guideTouched(touched = false)
            is TouchCommand.GuideNodeTapped -> vm.guideNodeMenu = CanvasViewModel.GuideNodeMenu(c.point?.toInt(), Offset(c.x, c.y))
        }
    }

    // the shared touches' waits (a hold, a repeat, an airbrush held still), run when they run out;
    // a fault in the core then costs that touch, not the app, as one while it takes a touch does
    private val touchTick = Runnable {
        val s = session ?: return@Runnable
        try {
            obey(s.touchTick(android.os.SystemClock.uptimeMillis().toDouble()))
            scheduleTouchTick(s)
        } catch (fault: io.github.artcraftandroid.procraft.core.InternalException) {
            AppLog.e("touch", "a touch's wait failed in the core; its touches let go", fault)
            runCatching { s.touchCancel() }
        }
    }

    private fun scheduleTouchTick(s: CanvasSession) {
        removeCallbacks(touchTick)
        val due = s.touchDeadline() ?: return
        postDelayed(touchTick, (due - android.os.SystemClock.uptimeMillis()).toLong().coerceAtLeast(0L))
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(e: MotionEvent): Boolean {
        val s = session ?: return true
        if (penButton(e)) return true
        predictor.record(e)
        if (e.actionMasked == MotionEvent.ACTION_DOWN) {
            // the strokes' sound, resting while none was drawn, wakes for this one
            vm.canvasTouched()
            // Continue filling on a layer that takes no fill: the dialog says why, and the touch does nothing
            if (vm.dropBand == CanvasViewModel.DropBand.FILLING && !vm.tapFillAllowed()) {
                skipping = true
                return true
            }
            // a mouse's other buttons do nothing on the canvas: only its primary button paints
            skipping = kindOf(e, 0) == PointerKind.MOUSE && !mousePressPaints(e.buttonState)
            if (skipping) return true
            // a touch on the canvas closes an adjustment's menu, its Layer / Pencil choice and liquify's popups
            closedAdjustMenu = vm.adjustMenuAt != null
            if (vm.adjustment != null) {
                vm.adjustMenuAt = null
                vm.adjustModesOpen = false
                vm.liquifyPopup = 0
            }
            // every sample as it arrives, not batched to the frame
            requestUnbufferedDispatch(e)
            s.setTouchSettings(vm.touchSettings())
            // with a panel open, the touch waits to be a tap or a drag
            s.setTouchPanelOpen(vm.panel != Panel.NONE)
            // Continue filling: each touch fills where it lands; slid, it changes the threshold
            s.setTouchFilling(vm.touchFill())
            // the selection tool's touch selects as the tool is set
            s.setTouchSelecting(vm.selectTouch())
            if (vm.selecting) vm.selectionPopup = SelectionPopup.NONE
            // a transform's touch takes the node it lands on (the warp's inner points with the advanced mesh)
            s.setTouchTransforming(if (vm.transforming) io.github.artcraftandroid.procraft.core.TransformTouch(vm.advancedMesh) else null)
            // the guide editor's touch drags a handle; fingers only move the view
            s.setTouchGuideEditing(vm.guideEditing)
            // the companion's Zoom or Move: a lone press zooms or moves the canvas
            s.setTouchCompanion(
                when (vm.companionMode) {
                    CanvasViewModel.CompanionMode.ZOOM -> CompanionMode.ZOOM
                    CanvasViewModel.CompanionMode.MOVE -> CompanionMode.MOVE
                    CanvasViewModel.CompanionMode.NONE -> null
                },
            )
        }
        if (skipping) return true
        // Liquify's brush shows as a ring of its reach where the touch presses, until it lifts
        if (vm.adjustment == Adjustment.LIQUIFY) {
            val lifted = e.actionMasked == MotionEvent.ACTION_UP || e.actionMasked == MotionEvent.ACTION_CANCEL
            vm.showLiquifyRing(if (lifted || e.pointerCount > 1) null else Offset(e.x, e.y))
        }
        // a fault in the core as it takes a touch costs that touch, not the app: it goes in the log
        // and the touches start afresh
        try {
            route(e, s)
        } catch (fault: io.github.artcraftandroid.procraft.core.InternalException) {
            io.github.artcraftandroid.procraft.AppLog.e("touch", "a touch failed in the core; its touches let go", fault)
            runCatching { s.touchCancel() }
        }
        // Layer opacity: the shared touches drag it; the band shows it as they go
        if (vm.opacityLayer != null) vm.layerOpacityTouched(e.actionMasked == MotionEvent.ACTION_UP || e.actionMasked == MotionEvent.ACTION_CANCEL)
        return true
    }

    /** The touch went down where Continue filling takes no fill: the rest of it does nothing. */
    private var skipping = false

    /** The touch went down with an adjustment's menu open: a tap only closes it. */
    private var closedAdjustMenu = false

    /**
     * The pen's side buttons, hovering or touching: what the shared touches say Gesture controls set
     * them to do (the first, set to nothing, switches to the eraser and back; set to erase, it
     * erases while held).
     */
    private fun penButton(e: MotionEvent): Boolean {
        if (isPen(e.getToolType(0), e.source)) penAt = Offset(e.x, e.y)
        // by what the event says is held: some pens' drivers send no press of their own, only their
        // hover and touch events with the button in them
        val penIn = (0 until e.pointerCount).any { isPen(e.getToolType(it), e.source) }
        val a = e.actionMasked
        val (pressed, letGo) = penButtons.next(e.buttonState, penIn, penTouching(a), penLifting(a), penLeaving(a))
        val own = e.actionMasked == MotionEvent.ACTION_BUTTON_PRESS || e.actionMasked == MotionEvent.ACTION_BUTTON_RELEASE
        // let go, whatever is open: one held to erase gives the eraser back (before this event's
        // touch, so a stroke that begins as it is let go paints)
        session?.let { s ->
            if (letGo and FIRST_BUTTON != 0) penLetGo(s, second = false)
            if (letGo and SECOND_BUTTON != 0) penLetGo(s, second = true)
        }
        if (pressed == 0) return own && penIn && (e.actionButton and PEN_BUTTONS) != 0
        // under the brush studio the tool would change unseen
        if (vm.studioOpen) return own
        val s = session ?: return own
        // both at once (one event for the two): each does what it is set to
        if (pressed and FIRST_BUTTON != 0) penPressed(s, second = false, e.x, e.y)
        if (pressed and SECOND_BUTTON != 0) penPressed(s, second = true, e.x, e.y)
        return own
    }

    /** A side button went down with the pen at ([x], [y]); a fault in the core costs the press, not the app. */
    private fun penPressed(s: CanvasSession, second: Boolean, x: Float, y: Float) = penFault {
        // as they are now: nothing has told the session if no touch has come since they were set
        s.setTouchSettings(vm.touchSettings())
        obey(s.touchPenButton(second, x, y))
        vm.penButtonDone()
    }

    /** A side button let go: one held to erase gives the eraser back. */
    private fun penLetGo(s: CanvasSession, second: Boolean) = penFault {
        if (s.touchPenButtonUp(second)) vm.penButtonDone()
    }

    /** Runs [act], a pen button's: a fault in the core goes in the log, as one in a touch does. */
    private inline fun penFault(act: () -> Unit) {
        try {
            act()
        } catch (fault: io.github.artcraftandroid.procraft.core.InternalException) {
            AppLog.e("touch", "a pen button failed in the core", fault)
        }
    }

    /**
     * A side button that comes as a key, pressed or let go, or a double-tap that does: at the pen, or
     * the middle of the canvas before it has shown. A double-tap is never held, so set to erase
     * while held it switches to the eraser and back, as when left alone.
     */
    private val penKeyHeard: (Boolean, PenKeyAction) -> Unit = { second, action ->
        val at = penAt ?: Offset(width / 2f, height / 2f)
        session?.let { s ->
            when (action) {
                PenKeyAction.PRESSED -> penPressed(s, second, at.x, at.y)
                PenKeyAction.LET_GO -> penLetGo(s, second)
                PenKeyAction.DOUBLE_TAPPED -> penFault {
                    s.setTouchSettings(vm.touchSettings())
                    obey(s.touchPenDoubleTap(PenPreferred.SWITCH_ERASER, at.x, at.y))
                    vm.penButtonDone()
                }
            }
        }
    }

    /** Touches passed on by what covers the canvas (the guide editor), in the canvas's own place. */
    private val passedTouch: (MotionEvent) -> Boolean = { e -> onTouchEvent(e) }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        vm.penKeyHeard = penKeyHeard
        vm.canvasTouch = passedTouch
    }

    override fun onDetachedFromWindow() {
        if (vm.penKeyHeard === penKeyHeard) vm.penKeyHeard = null
        if (vm.canvasTouch === passedTouch) vm.canvasTouch = null
        super.onDetachedFromWindow()
    }

    override fun onGenericMotionEvent(e: MotionEvent): Boolean = penButton(e) || super.onGenericMotionEvent(e)

    /** The pen or the mouse hovering over the canvas shows the brush cursor where it would paint. */
    override fun onHoverEvent(e: MotionEvent): Boolean = try {
        hoverEvent(e)
    } catch (fault: io.github.artcraftandroid.procraft.core.InternalException) {
        // as for a touch: the hover is let go, the app keeps going, the log says why
        io.github.artcraftandroid.procraft.AppLog.e("touch", "a pen's hover failed in the core", fault)
        true
    }

    private fun hoverEvent(e: MotionEvent): Boolean {
        tellPointer(e, 0, "hovering")
        if (penButton(e)) return true
        if (kindOf(e, 0) == PointerKind.MOUSE) return mouseHover(e)
        if (kindOf(e, 0) != PointerKind.STYLUS) return super.onHoverEvent(e)
        when (e.actionMasked) {
            MotionEvent.ACTION_HOVER_ENTER, MotionEvent.ACTION_HOVER_MOVE -> {
                // (told each time: a session opened meanwhile knows it too); a hand already down is set aside
                session?.setTouchPenHovering(true)?.let(::obey)
                hoverPen = io.github.artcraftandroid.procraft.core.CursorPen(e.getOrientation(0), 0f)
                hovered(Offset(e.x, e.y))
            }
            MotionEvent.ACTION_HOVER_EXIT -> {
                session?.setTouchPenHovering(false)?.let(::obey)
                penHovering = false
                vm.showCursor(null)
                vm.showLiquifyRing(null)
            }
        }
        return true
    }

    /**
     * A mouse (or a touchpad's pointer) moving over the canvas with no button held: the brush
     * cursor follows it, as it does the pen held over the glass, but it is no pen near (a hand
     * landing then still paints, and Hover's touch stays the pen's).
     */
    private fun mouseHover(e: MotionEvent): Boolean {
        val at = Offset(e.x, e.y)
        val gone = e.actionMasked == MotionEvent.ACTION_HOVER_EXIT || vm.overInterface(at)
        if (gone || !painting()) vm.showCursor(null) else vm.showCursor(at, hovering = true)
        if (vm.adjustment == Adjustment.LIQUIFY) vm.showLiquifyRing(at.takeUnless { gone })
        return true
    }

    /** The pen is held over the canvas at [at]: hovering, or touching at no pressure before it presses. */
    private fun hovered(at: Offset) {
        // a spray can's sound (a plugin's) rattles shaken above the canvas
        if (vm.strokeSoundOn) session?.soundHover(at.x, at.y)
        penHovering = true
        // not over the bars or an open panel, where the gaps between their buttons and rows come
        // here and the buttons don't: it came and went as the pen crossed them, drawn under them
        val over = vm.overInterface(at)
        if (over) vm.showCursor(null) else if (painting()) vm.showCursor(at, hovering = true, pen = hoverPen)
        // and Liquify's ring where the pen hovers
        if (vm.adjustment == Adjustment.LIQUIFY) vm.showLiquifyRing(at.takeUnless { over })
    }

    /** Whether a pen on the canvas now would paint (no tool of another kind open). */
    private fun painting() = vm.adjustment == null && !vm.selecting && !vm.transforming

    /** Input stopped coming, so the finger or pen has stopped: nothing runs ahead of it any more. */
    private val predictionOver = Runnable { session?.strokePredict(emptyList()) }

    /** Draws the predicted next samples of the stroke ahead of it (replaced at the next input). */
    private fun predict(s: CanvasSession) {
        removeCallbacks(predictionOver)
        val p = predictor.predict() ?: return
        try {
            val i = p.findPointerIndex(strokePointer)
            if (i >= 0) s.strokePredict(samples(p, i))
        } finally {
            p.recycle()
        }
        postDelayed(predictionOver, PREDICTION_LASTS_MS)
    }

    private fun kindOf(e: MotionEvent, index: Int): PointerKind = pointerKindOf(e, index)

    /** The kinds of pointer told in the log so far. */
    private val pointersTold = PointersTold()

    /** Each device's first pen stroke, summed up in the log as it lifts, and its pointer. */
    private val penStrokes = PenStrokesTold()
    private var summedPointer = -1

    /** The pressures of pointer [i]'s samples in [e], into the first pen stroke's sum. */
    private fun sumPressures(e: MotionEvent, i: Int) {
        for (h in 0 until e.historySize) penStrokes.sample(e.getHistoricalPressure(i, h))
        penStrokes.sample(e.getPressure(i))
    }

    /**
     * Pointer [i] of [e], [what] it does (down, or hovering): the first of its kind from its device
     * is told in the log, with what a report of a pen that doesn't paint needs.
     */
    private fun tellPointer(e: MotionEvent, i: Int, what: String) {
        val toolType = e.getToolType(i)
        if (!pointersTold.isNew(e.deviceId, toolType, e.source)) return
        val device = e.device
        // (a pen paired over Bluetooth may carry a name its owner gave it: only a built-in one's)
        val builtIn = device != null && android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.Q && !device.isExternal
        val named = if (builtIn) " \"${device?.name}\"" else ""
        val about = device?.let { "device %04x:%04x%s".format(java.util.Locale.ROOT, it.vendorId, it.productId, named) } ?: "no device"
        val axis = device?.getMotionRange(MotionEvent.AXIS_PRESSURE, e.source)?.let { it.min..it.max }
        AppLog.i(TAG, pointerFacts(what, about, toolType, e.source, kindOf(e, i), e.getPressure(i), axis, e.buttonState, e.getTouchMajor(i)))
        // every axis a pen's device says it gives (a barrel's turn has no axis of Android's own)
        if (isPen(toolType, e.source)) {
            val axes = device?.motionRanges.orEmpty().filter { it.source and e.source != 0 }.map { MotionEvent.axisToString(it.axis) }.distinct()
            AppLog.i(TAG, "its axes: ${axes.joinToString().ifEmpty { "none said" }}")
        }
    }

    /** The pointer's samples in this event: the batched ones since the last event, then the latest. */
    private fun samples(e: MotionEvent, i: Int): List<PenSample> {
        val out = ArrayList<PenSample>(e.historySize + 1)
        for (h in 0 until e.historySize) {
            out += PenSample(
                e.getHistoricalX(i, h),
                e.getHistoricalY(i, h),
                e.getHistoricalPressure(i, h),
                penLean(e.getHistoricalAxisValue(MotionEvent.AXIS_TILT, i, h), e.getHistoricalOrientation(i, h)),
                e.getHistoricalOrientation(i, h),
                e.getHistoricalEventTime(h).toDouble(),
            )
        }
        out += PenSample(e.getX(i), e.getY(i), e.getPressure(i), penLean(e.getAxisValue(MotionEvent.AXIS_TILT, i), e.getOrientation(i)), e.getOrientation(i), e.eventTime.toDouble())
        return out
    }

    private companion object {
        const val TAG = "HanaCreate"

        /** How long a prediction stands with no input after it, milliseconds: a few input intervals. */
        const val PREDICTION_LASTS_MS = 40L
    }
}
