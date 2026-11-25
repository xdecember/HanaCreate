package io.github.artcraftandroid.procraft

import android.graphics.Color
import android.hardware.display.DisplayManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.runtime.snapshotFlow
import androidx.lifecycle.lifecycleScope
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.ProjectionPresentation
import io.github.artcraftandroid.procraft.canvas.PEN_DOUBLE_TAP_KEY
import io.github.artcraftandroid.procraft.canvas.systemShortcutKeys
import io.github.artcraftandroid.procraft.canvas.PenKeyAction
import io.github.artcraftandroid.procraft.canvas.PenDoubleTaps
import io.github.artcraftandroid.procraft.canvas.keyPress
import io.github.artcraftandroid.procraft.canvas.KeysTold
import io.github.artcraftandroid.procraft.canvas.maybePenKeys
import io.github.artcraftandroid.procraft.canvas.penKeyButton
import io.github.artcraftandroid.procraft.canvas.pointerKindOf
import kotlinx.coroutines.launch
import io.github.artcraftandroid.procraft.ui.AppRoot

/** How long a finger's touch stays where it landed with tap assistance on. */
private const val TAP_HOLD_MS = 300L

/** How far it may drift meanwhile and still be held (dp): farther, it is a scroll or a drag and moves. */
private const val TAP_DRIFT_DP = 24f

class MainActivity : ComponentActivity() {
    private val vm: CanvasViewModel by viewModels()

    /** Hides the system's bars, as the model asks (let go as this activity goes: the model outlives it). */
    private val barsHider: () -> Unit = { hideSystemBars() }

    /**
     * The brush library's file picker, registered with the activity rather than the library: what
     * is picked comes in even if the library closed while the picker was up, or the system ended
     * the app meanwhile (it is registered again as the activity is made again, and the pick is
     * handed to it then). Any kind of file can be picked: brush files have no type of their own.
     */
    private val brushFilePicker = registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let(vm::importBrushes)
    }
    private val brushPickerOpener: () -> Unit = {
        runCatching { brushFilePicker.launch(arrayOf("*/*")) }.onFailure { AppLog.e("HanaCreate", "opening the brush file picker", it) }
    }

    // Project canvas: the presentation on a second display, and the displays coming and going
    private var projection: ProjectionPresentation? = null
    private val displays by lazy { getSystemService(DisplayManager::class.java) }
    private val displayListener = object : DisplayManager.DisplayListener {
        override fun onDisplayAdded(displayId: Int) = showProjection()
        override fun onDisplayRemoved(displayId: Int) = showProjection()
        override fun onDisplayChanged(displayId: Int) = Unit
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(SystemBarStyle.dark(Color.TRANSPARENT), SystemBarStyle.dark(Color.TRANSPARENT))
        super.onCreate(savedInstanceState)
        // the log's file first, so that what follows is in it
        AppLog.start(this)
        // which graphics backend the app is trying, kept before the model (made at its first use
        // below) makes the GPU device: a driver that crashes the app then sends the next start to
        // the next backend (one that drew is trusted until the app or the system is updated); and
        // how the last runs ended, so that a driver that crashed them while drawing sends this one
        // to the other backend
        io.github.artcraftandroid.procraft.core.setGpuRecordFolder(filesDir.path, installedBuild())
        AppLog.toGpuStart(this)
        // text's fonts given to the core, off the main thread
        io.github.artcraftandroid.procraft.canvas.TextRenderer.prepare(assets, java.io.File(filesDir, "fonts"))
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) window.isNavigationBarContrastEnforced = false
        hideSystemBars()
        // a colour dragged down from the top bar puts back the bars the drag showed
        vm.hideSystemBars = barsHider
        vm.pickBrushFile = brushPickerOpener
        // made here, not inside the flow's read-only snapshot, where its setup can't write state
        val model = vm
        // laid out left to right in every language, as the original is: a right-to-left one
        // (Arabic, Hebrew) would otherwise mirror the bars
        setContent {
            androidx.compose.runtime.CompositionLocalProvider(
                androidx.compose.ui.platform.LocalLayoutDirection provides androidx.compose.ui.unit.LayoutDirection.Ltr,
            ) { AppRoot(model) }
        }
        lifecycleScope.launch { snapshotFlow { model.projectCanvas }.collect { showProjection() } }
        // wide colour while a Display P3 canvas is open, so its colours show as they are around it
        // too; else the default, as the canvas's surface then is: a wide window has had the
        // interface over the canvas blurred away, or the canvas dulled, on some tablets' screens
        lifecycleScope.launch {
            snapshotFlow { model.wideColorWindow }.collect { wide ->
                window.colorMode = if (wide) android.content.pm.ActivityInfo.COLOR_MODE_WIDE_COLOR_GAMUT else android.content.pm.ActivityInfo.COLOR_MODE_DEFAULT
            }
        }
        importOpened(intent)
    }

    /** The app as installed and the system it runs on, as one text: it differs once either is updated. */
    private fun installedBuild(): String {
        val installed = runCatching { packageManager.getPackageInfo(packageName, 0).lastUpdateTime }.getOrDefault(0L)
        return "$installed ${Build.FINGERPRINT}"
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        importOpened(intent)
    }

    /**
     * A file opened with the app, or shared to it, is imported into the gallery (once), which
     * shows: an open artwork is saved and closed first.
     */
    private fun importOpened(intent: android.content.Intent?) {
        intent ?: return
        // the home screen's Recent artwork widget: straight into the newest artwork
        if (intent.action == io.github.artcraftandroid.procraft.gallery.RecentArtworkWidget.OPEN_RECENT) {
            intent.action = null
            vm.openMostRecent()
            return
        }
        val uri = when (intent.action) {
            android.content.Intent.ACTION_VIEW -> intent.data
            android.content.Intent.ACTION_SEND ->
                androidx.core.content.IntentCompat.getParcelableExtra(intent, android.content.Intent.EXTRA_STREAM, android.net.Uri::class.java)
            else -> null
        } ?: return
        intent.action = null
        // a plugin is asked about where the app stands: it needs no artwork closed
        if (vm.isPluginFile(uri)) {
            vm.offerPlugin(uri)
            return
        }
        // the artwork open is left (saved) first; one that can't be saved stays, and the file comes in
        // only if it is left without saving
        if (vm.screen == io.github.artcraftandroid.procraft.canvas.Screen.CANVAS) vm.closeArtwork { vm.importFile(uri) } else vm.importFile(uri)
    }

    override fun onStart() {
        super.onStart()
        vm.readAppSettings()
        vm.resumeTracking()
        vm.appInFront(true)
        displays.registerDisplayListener(displayListener, null)
        showProjection()
    }

    override fun onStop() {
        super.onStop()
        vm.appInFront(false)
        vm.saveNow()
        displays.unregisterDisplayListener(displayListener)
        projection?.dismiss()
        projection = null
    }

    /** The system short of memory, or the app out of sight: the canvas lets go of what it keeps only to be quicker. */
    @Suppress("DEPRECATION")
    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        if (level >= android.content.ComponentCallbacks2.TRIM_MEMORY_RUNNING_LOW) {
            vm.session?.trimMemory()
            // the library's sample strokes too: the rows showing draw theirs again from the core's copy
            vm.letGoOfPreviews()
        }
    }

    /** Project canvas: while it is on, the canvas shows on the first second display there is. */
    private fun showProjection() {
        val display = if (vm.projectCanvas) displays.getDisplays(DisplayManager.DISPLAY_CATEGORY_PRESENTATION).firstOrNull() else null
        if (projection != null && projection?.display?.displayId == display?.displayId) return
        projection?.dismiss()
        projection = display?.let { d -> ProjectionPresentation(this, d, vm).takeIf { runCatching { it.show() }.isSuccess } }
    }

    // tap assistance: where a finger first landed, and when, while its touch is held there
    private var heldAt: android.graphics.PointF? = null
    private var heldSince = 0L

    /**
     * Tap assistance: for its first moments a finger's touch stays where it landed (its moves are
     * held back and its lift is put back there), so a tap that drifts still lands where it began.
     * More fingers, a touch held longer, or one gone farther than a drift (a list scrolled) go on
     * as they are.
     */
    override fun dispatchTouchEvent(e: android.view.MotionEvent): Boolean {
        // (a mouse or a touchpad moving the pointer is no finger, though a touchpad's says it is)
        if (!vm.tapAssistance || pointerKindOf(e, 0) != io.github.artcraftandroid.procraft.core.PointerKind.FINGER) return super.dispatchTouchEvent(e)
        when (e.actionMasked) {
            android.view.MotionEvent.ACTION_DOWN -> {
                heldAt = android.graphics.PointF(e.x, e.y)
                heldSince = e.eventTime
            }
            android.view.MotionEvent.ACTION_POINTER_DOWN, android.view.MotionEvent.ACTION_CANCEL -> heldAt = null
            android.view.MotionEvent.ACTION_MOVE -> {
                val at = heldAt
                val drift = TAP_DRIFT_DP * resources.displayMetrics.density
                if (at != null && e.eventTime - heldSince <= TAP_HOLD_MS && kotlin.math.hypot(e.x - at.x, e.y - at.y) <= drift) return true
                heldAt = null
            }
            android.view.MotionEvent.ACTION_UP -> {
                val at = heldAt
                heldAt = null
                if (at != null) {
                    val held = android.view.MotionEvent.obtain(e).apply { setLocation(at.x, at.y) }
                    return super.dispatchTouchEvent(held).also { held.recycle() }
                }
            }
        }
        return super.dispatchTouchEvent(e)
    }

    private val penDoubleTaps = PenDoubleTaps()
    private val keysTold = KeysTold()

    /**
     * A hardware keyboard's shortcuts on the canvas, a tablet's express keys' too (keys no text
     * field took). Ctrl and Meta are both the command key.
     */
    override fun onKeyDown(keyCode: Int, event: android.view.KeyEvent): Boolean {
        // a pen's double-tap that comes as a key of its own: as its first button, once for each
        if (keyCode == PEN_DOUBLE_TAP_KEY) {
            if (event.repeatCount == 0 && penDoubleTaps.isNew(event.eventTime)) vm.penKey(false, PenKeyAction.DOUBLE_TAPPED)
            return true
        }
        // a pen's side button that comes as a key
        val penButton = event.device?.let { penKeyButton(keyCode, it.sources or event.source, it.keyboardType, it.name) }
        if (penButton != null && (event.repeatCount > 0 || vm.penKey(penButton))) return true
        // any other key a pen sends, told once: what a pen's button gives that isn't taken
        event.device?.takeIf { penButton == null && maybePenKeys(it.sources or event.source, it.name) && keysTold.isNew(it.id, keyCode) }?.let {
            AppLog.i("keys", "a key from a pen: ${android.view.KeyEvent.keyCodeToString(keyCode)}, meta 0x%x, from %s (source 0x%x)".format(java.util.Locale.ROOT, event.metaState, it.name, event.source))
        }
        // typing in a text field (a name, the text tool's words): what the field leaves (Ctrl+J,
        // Alt) is still typing, not the canvas's
        if (typing()) return super.onKeyDown(keyCode, event)
        if (isAltKey(keyCode) && event.repeatCount == 0 && !event.isCtrlPressed && vm.modifyKey(true)) return true
        val press = keyPress(keyCode, event.metaState)
        // held, a key goes on moving the view, if that is what it does
        if (press != null && vm.keyShortcut(press, repeat = event.repeatCount > 0)) return true
        return super.onKeyDown(keyCode, event)
    }

    /** Whether a text field has the keys: the view with focus edits text (Compose's, while one of its fields is focused). */
    private fun typing() = currentFocus?.onCheckIsTextEditor() == true

    override fun onKeyUp(keyCode: Int, event: android.view.KeyEvent): Boolean {
        // a pen's side button that comes as a key, let go (held to erase, the eraser goes back)
        val penButton = event.device?.let { penKeyButton(keyCode, it.sources or event.source, it.keyboardType, it.name) }
        if (penButton != null && vm.penKey(penButton, PenKeyAction.LET_GO)) return true
        if (isAltKey(keyCode) && vm.modifyHeld && vm.modifyKey(false)) return true
        return super.onKeyUp(keyCode, event)
    }

    private fun isAltKey(keyCode: Int) = keyCode == android.view.KeyEvent.KEYCODE_ALT_LEFT || keyCode == android.view.KeyEvent.KEYCODE_ALT_RIGHT

    /** The keyboard shortcuts for the system's list of them (Meta+/, or Help's Keyboard shortcuts): the core's, with the keys a plugin rebinding them says do each now. */
    override fun onProvideKeyboardShortcuts(
        data: MutableList<android.view.KeyboardShortcutGroup>,
        menu: android.view.Menu?,
        deviceId: Int,
    ) {
        super.onProvideKeyboardShortcuts(data, menu, deviceId)
        for (group in vm.plugins.keyShortcutList(false, false)) {
            val items = group.rows.flatMap { row ->
                row.combos.mapNotNull { c ->
                    val (code, modifiers) = systemShortcutKeys(c.key, c.command, c.shift, c.alt) ?: return@mapNotNull null
                    android.view.KeyboardShortcutInfo(row.does, code, modifiers)
                }
            }
            if (items.isNotEmpty()) data.add(android.view.KeyboardShortcutGroup(group.heading, items))
        }
    }

    override fun onDestroy() {
        if (vm.hideSystemBars === barsHider) vm.hideSystemBars = null
        if (vm.pickBrushFile === brushPickerOpener) vm.pickBrushFile = null
        super.onDestroy()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus) return
        hideSystemBars()
        // what another app copied meanwhile (the clipboard can only be looked at with the focus)
        vm.lookAtSystemClipboard()
    }

    /** The canvas uses the whole screen; a swipe from the edge shows the bars for a moment. */
    private fun hideSystemBars() {
        val controller = WindowCompat.getInsetsController(window, window.decorView)
        controller.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        controller.hide(WindowInsetsCompat.Type.systemBars())
    }
}
