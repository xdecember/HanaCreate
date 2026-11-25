package io.github.artcraftandroid.procraft.ui

import android.content.Context
import android.content.Intent
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.awaitLongPressOrCancellation
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.foundation.border
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChangeIgnoreConsumed
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.FileProvider
import androidx.lifecycle.viewModelScope
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.label
import io.github.artcraftandroid.procraft.core.QuickAction
import io.github.artcraftandroid.procraft.core.PluginControl
import io.github.artcraftandroid.procraft.core.PluginFile
import io.github.artcraftandroid.procraft.core.PluginInfo
import io.github.artcraftandroid.procraft.core.PluginSlider
import io.github.artcraftandroid.procraft.core.PluginSource
import io.github.artcraftandroid.procraft.core.pluginSliderAlong
import io.github.artcraftandroid.procraft.core.pluginSliderAt
import io.github.artcraftandroid.procraft.core.pluginSliderChoice
import io.github.artcraftandroid.procraft.core.pluginSliderChoices
import io.github.artcraftandroid.procraft.core.pluginSliderChosen
import io.github.artcraftandroid.procraft.core.pluginSliderShown
import kotlinx.coroutines.launch
import kotlin.math.roundToInt
import java.io.File
import io.github.artcraftandroid.procraft.core.PanelRow

private val ActionsWidth = 390.dp

/** One row of the actions panel. Rows whose features are still to come show dimmed. */
private sealed interface ActionRow

/** A button row; with a [private] label, a swipe to the left shows a second button doing it privately. */
private class Act(
    val label: String,
    val value: String? = null,
    val private: String? = null,
    val onPrivate: (() -> Unit)? = null,
    val onClick: (() -> Unit)? = null,
) : ActionRow

/** A switch row; [fixed] is a setting shown as it is, its name not dimmed. */
private class Toggle(val label: String, val checked: Boolean, val fixed: Boolean = false, val onChange: ((Boolean) -> Unit)? = null) : ActionRow

private class Heading(val label: String) : ActionRow

/** A setting with its value on the right and a slider under its name, always shown. */
private class Slide(val label: String, val value: String, val fraction: Float, val tall: Dp = 63.dp, val onDone: () -> Unit = {}, val onChange: (Float) -> Unit) : ActionRow

/** The pages the panel shows: its tabs, or a page of settings reached from one of them. */
private enum class ActionsPage { TABS, PRESSURE, CURSOR, PLUGINS, PEN_BUTTONS }

private val CurveFill get() = if (Pc.light) Color(0xFFE7E7E9) else Color(0xFF262626)

/** Advanced cursor's section headings: bold, as large as the rows, lighter than the tabs' names. */
private val CursorHeading get() = if (Pc.light) Pc.TabIdle else Color(0xFFB5B5B5)
private val CurveEdge get() = if (Pc.light) Color(0xFF999999) else Color(0xFF666666)
private val CurveGrid get() = if (Pc.light) Color(0xFFCACACA) else Color(0xFF353535)
private val CurveLine get() = if (Pc.light) Color.Black else Color.White

/** A fraction shown as the panel's sliders show it: "None" at nothing, else a percentage. */
private fun percent(v: Float) = if (v <= 0f) "None" else "%.1f%%".format(v * 100f)

private class Tab(val label: String, val icon: ImageVector)

private val Tabs = listOf(
    Tab("Add", PcIcons.TabAdd),
    Tab("Canvas", PcIcons.TabCanvas),
    Tab("Share", PcIcons.TabShare),
    Tab("Video", PcIcons.TabVideo),
    Tab("Prefs", PcIcons.TabPrefs),
    Tab("Help", PcIcons.TabHelp),
)

/** On a model, its own tab takes Video's place. */
private val ModelTabs = Tabs.map { if (it.label == "Video") Tab("3D", PcIcons.Tab3D) else it }

/** The actions panel: six tabs of grouped rows (adding, the canvas, sharing, video, preferences, help). */
@Composable
fun ActionsPanel(vm: CanvasViewModel, arrowX: Dp, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var dialog by remember { mutableStateOf<Pair<String, String>?>(null) }
    var exportingVideo by remember { mutableStateOf(false) }
    fun shareVideo(seconds: Int?) {
        exportingVideo = false
        if (vm.timeLapseFrames() == 0) {
            dialog = "Time-lapse" to "There is no time-lapse to export yet."
            return
        }
        // one at a time: the one under way shares when it's made
        if (vm.timeLapseExporting) return
        // made outside the panel, which may be closed meanwhile: the share comes all the same
        // (cancelled or failed, there is nothing to share; a failure is told)
        vm.viewModelScope.launch {
            vm.exportTimeLapse(seconds)?.let { video ->
                runCatching { shareFile(context, video, "video/mp4") }
                    .onFailure { io.github.artcraftandroid.procraft.AppLog.e("HanaCreate", "sharing the time-lapse video (${video.length()} bytes)", it) }
            }
        }
    }
    var page by remember { mutableStateOf(ActionsPage.TABS) }
    // the tab whose rows are shown: changing tabs, the old rows fade out, then the new ones fade in
    var shownTab by remember { mutableStateOf(vm.actionsTab) }
    val rowsShown = remember { androidx.compose.animation.core.Animatable(1f) }
    LaunchedEffect(vm.actionsTab) {
        if (shownTab == vm.actionsTab) return@LaunchedEffect
        rowsShown.animateTo(0f, Motion.actionsTabOut())
        shownTab = vm.actionsTab
        rowsShown.animateTo(1f, Motion.actionsTabIn())
    }
    // Brush scaling's menu is open
    var scalingMenu by remember { mutableStateOf(false) }
    // a PDF waiting on its quality
    var pdfAsking by remember { mutableStateOf<CanvasViewModel.Export?>(null) }
    // made outside the panel, which may be closed meanwhile; cancelled or failed (that is told),
    // nothing is shared
    fun share(kind: CanvasViewModel.Export, pdfQuality: Int = 100) {
        vm.viewModelScope.launch {
            val files = vm.export(kind, pdfQuality)
            if (files.isNotEmpty()) runCatching { shareFiles(context, files, kind.mime) }
        }
    }
    // the layered package, named with ours or the other app's extension
    fun sharePackage(extension: String) {
        vm.viewModelScope.launch {
            vm.exportPackageFile(extension)?.let { runCatching { shareFile(context, it, "application/octet-stream") } }
        }
    }
    fun share(jpeg: Boolean) {
        vm.viewModelScope.launch {
            vm.exportImage(jpeg)?.let { runCatching { shareFile(context, it, if (jpeg) "image/jpeg" else "image/png") } }
        }
    }
    // a file a plugin makes of the artwork's layers of a kind it adds
    fun share(e: io.github.artcraftandroid.procraft.core.PluginExport) {
        vm.viewModelScope.launch {
            vm.exportPlugin(e)?.let { runCatching { shareFile(context, it, e.mime) } }
        }
    }
    // whether the picture being picked or taken goes on a private layer
    var privately by remember { mutableStateOf(false) }
    val pickPhoto = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia()) { uri ->
        uri?.let { vm.insertPicture(it, privately) }
    }
    val pickFile = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { vm.insertPicture(it, privately) }
    }
    // the camera writes its picture to a file of ours, which goes in like a photo
    val photo = remember {
        val dir = File(context.cacheDir, "exports").apply { mkdirs() }
        FileProvider.getUriForFile(context, context.packageName + ".files", File(dir, "camera.jpg"))
    }
    val takePhoto = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.TakePicture()) { taken ->
        if (taken) vm.insertPicture(photo, privately)
    }
    fun file(private: Boolean) {
        privately = private
        // a video's frames come in too
        pickFile.launch(arrayOf("image/*", "video/*"))
    }
    fun photoFrom(private: Boolean) {
        privately = private
        pickPhoto.launch(androidx.activity.result.PickVisualMediaRequest(androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia.ImageAndVideo))
    }
    fun camera(private: Boolean) {
        privately = private
        runCatching { takePhoto.launch(photo) }.onFailure { dialog = "Take a photo" to "No camera app is available." }
    }
    val model = vm.modelOpen
    val tabs = if (model) ModelTabs else Tabs
    val groups: List<List<ActionRow>> = if (model && shownTab <= 3) modelGroups(vm, shownTab, { k, q -> share(k, q) }, pickFile = { pickFile.launch(arrayOf("image/*")) },
        pickPhoto = { pickPhoto.launch(androidx.activity.result.PickVisualMediaRequest(androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia.ImageOnly)) },
        takePhoto = { runCatching { takePhoto.launch(photo) }.onFailure { dialog = "Take a photo" to "No camera app is available." } },
    ) else when (shownTab) {
        0 -> listOf(
            // swiped to the left, the first three and Paste put what they bring on a private layer
            listOf(
                Act("Insert a file", private = "Insert a private file", onPrivate = { file(true) }) { file(false) },
                Act("Insert a photo", private = "Insert a private photo", onPrivate = { photoFrom(true) }) { photoFrom(false) },
                Act("Take a photo", private = "Take a private photo", onPrivate = { camera(true) }) { camera(false) },
                Act("Add text") { vm.addText() },
            ),
            // Paste is grey until something has been cut or copied
            listOf(
                Act("Cut") { vm.cutLayer() },
                Act("Copy") { vm.copyLayer() },
                Act("Copy canvas") { vm.copyCanvas() },
                Act(
                    "Paste",
                    private = "Paste Private",
                    onPrivate = if (vm.canPaste) ({ vm.paste(private = true) }) else null,
                    onClick = if (vm.canPaste) ({ vm.paste() }) else null,
                ),
            ),
        )
        1 -> listOf(
            listOf(Act("Crop & Resize") { vm.beginCrop() }),
            listOf(
                Toggle("Animation Assist", vm.animation?.pages == false) { vm.changeAnimationAssist(it) },
                Toggle("Page Assist", vm.animation?.pages == true) { vm.changePageAssist(it) },
                Toggle("Drawing Guide", vm.guide?.on == true) { vm.setGuideOn(it) },
                // only with the guide on
                Act("Edit Drawing Guide", onClick = if (vm.guide?.on == true) ({ vm.openGuideEditor() }) else null),
            ),
            listOf(Toggle("Reference", vm.referenceOn) { vm.changeReferenceOn(it) }),
            listOf(Act("Flip horizontal") { vm.flipCanvas(horizontal = true) }, Act("Flip vertical") { vm.flipCanvas(horizontal = false) }),
            listOf(Act("Canvas information") { vm.canvasInfoOpen = true }),
        )
        2 -> listOf(
            listOfNotNull(
                Heading("Share Image"),
                // ours first, then the same package named for the other app
                Act(CanvasViewModel.OWN_LABEL) { sharePackage(CanvasViewModel.OWN_EXTENSION) },
                Act(CanvasViewModel.PACKAGE_LABEL) { sharePackage(CanvasViewModel.PACKAGE_EXTENSION) },
                Act("PSD") { share(CanvasViewModel.Export.PSD) },
                Act("PDF") { pdfAsking = CanvasViewModel.Export.PDF },
                Act("JPEG") { share(jpeg = true) },
                Act("PNG") { share(jpeg = false) },
                Act("TIFF") { share(CanvasViewModel.Export.TIFF) },
                // a plugin's artwork, its shapes kept
                Act("SVG") { share(CanvasViewModel.Export.SVG) }.takeIf { vm.session?.exportsSvg() == true },
                // files plugins make of the artwork's layers of kinds they add, under their own names
                *vm.session?.pluginExports().orEmpty().map { e -> Act(e.label) { share(e) } }.toTypedArray(),
            ),
            listOf(
                Heading("Share Layers"),
                Act("PDF") { pdfAsking = CanvasViewModel.Export.LAYERS_PDF },
                Act("PNG Files") { share(CanvasViewModel.Export.LAYER_PNGS) },
                Act("Animated GIF") { vm.animatedExport = CanvasViewModel.Animated.GIF },
                Act("Animated PNG") { vm.animatedExport = CanvasViewModel.Animated.PNG },
                Act("Animated MP4") { vm.animatedExport = CanvasViewModel.Animated.MP4 },
                Act("Animated HEVC") { vm.animatedExport = CanvasViewModel.Animated.HEVC },
            ),
        )
        3 -> listOf(
            listOf(
                // Replay and Export wait for something recorded
                Act(
                    "Time-lapse Replay",
                    onClick = {
                        // the panel fades away as the replay takes the screen
                        vm.fadePanels()
                        vm.timeLapseReplaying = true
                    }.takeIf { vm.timeLapseRecorded },
                ),
                Toggle("Time-lapse Recording", vm.timeLapseRecording) { vm.changeTimeLapseRecording(it) },
            ),
            listOf(Act("Export time-lapse video", onClick = { exportingVideo = true }.takeIf { vm.timeLapseRecorded })),
        )
        4 -> listOf(
            listOf(
                Toggle("Light interface", vm.lightInterface) { vm.changeLightInterface(it) },
                Toggle("Right-hand interface", vm.rightHanded) { vm.changeRightHanded(it) },
                Act("Brush scaling", value = if (vm.brushOnScreen) "Screen" else "Canvas") { scalingMenu = true },
                Toggle("Project canvas", vm.projectCanvas) { vm.changeProjectCanvas(it) },
                Toggle("Brush cursor", vm.brushCursorOn) { vm.changeBrushCursor(it) },
                Act("Advanced cursor settings") { page = ActionsPage.CURSOR },
            ),
            listOf(
                Act("Pen buttons") { page = ActionsPage.PEN_BUTTONS },
                Act("Pressure and Smoothing") { page = ActionsPage.PRESSURE },
                Act("Gesture controls") { vm.gestureControlsOpen = true },
                Act("Plugins") { page = ActionsPage.PLUGINS },
            ),
            listOf(
                Slide(
                    "Rapid undo delay",
                    "%.2fs".format(vm.rapidUndoDelay),
                    (vm.rapidUndoDelay - CanvasViewModel.RAPID_UNDO_MIN_S) / (CanvasViewModel.RAPID_UNDO_MAX_S - CanvasViewModel.RAPID_UNDO_MIN_S),
                ) { vm.changeRapidUndoDelay(CanvasViewModel.RAPID_UNDO_MIN_S + it * (CanvasViewModel.RAPID_UNDO_MAX_S - CanvasViewModel.RAPID_UNDO_MIN_S)) },
                Slide("Selection mask visibility", "%.1f%%".format(vm.selectionVisibility * 100f), vm.selectionVisibility, onChange = vm::changeSelectionVisibility),
            ),
            listOf(Toggle("Size & Opacity sidebar", vm.showSideBar) { vm.showSideBar = it }),
        )
        else -> listOf(
            listOf(
                Act("About HanaCreate") { dialog = "HanaCreate" to "Version ${versionName(context)}" },
                Act("Licences") { dialog = "Licences" to licences(context) },
            ),
            listOf(
                Act("Keyboard shortcuts") {
                    // the system's list of them, which the app fills in (MainActivity)
                    var c = context
                    while (c is android.content.ContextWrapper && c !is android.app.Activity) c = c.baseContext
                    (c as? android.app.Activity)?.requestShowKeyboardShortcuts()
                },
                Act("Advanced settings") {
                    // the app's own settings page, apart from the canvas
                    runCatching { context.startActivity(Intent(context, io.github.artcraftandroid.procraft.SettingsActivity::class.java)) }
                },
                Act("Share log") {
                    // made outside the panel, which may be closed meanwhile
                    shareLog(context, vm.viewModelScope) { dialog = "Share log" to LOG_NOT_READY }
                },
            ),
        )
    }
    Popover(ActionsWidth, arrowX, modifier) {
        androidx.compose.animation.AnimatedContent(
            page,
            transitionSpec = { Motion.panelPage(forward = targetState != ActionsPage.TABS) },
            label = "page",
        ) { shown ->
            Column {
                if (shown == ActionsPage.PRESSURE) {
                    PressurePage(vm) { page = ActionsPage.TABS }
                    return@Column
                }
                if (shown == ActionsPage.CURSOR) {
                    CursorPage(vm) { page = ActionsPage.TABS }
                    return@Column
                }
                if (shown == ActionsPage.PLUGINS) {
                    PluginsPage(vm) { page = ActionsPage.TABS }
                    return@Column
                }
                if (shown == ActionsPage.PEN_BUTTONS) {
                    PenButtonsPage(vm) { page = ActionsPage.TABS }
                    return@Column
                }
                Text(
                    "Actions",
                    color = Pc.PanelTitle,
                    fontSize = 24.sp,
                    fontWeight = FontWeight.Bold,
                    modifier = Modifier.padding(start = 24.dp, top = 22.dp).height(34.dp),
                )
                Row(Modifier.padding(start = 10.dp).height(60.dp)) {
                    tabs.forEachIndexed { i, tab ->
                        // a tab lights up as soon as it is touched; the old one goes out when it is let go
                        val touch = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }
                        val held = heldOf(touch)
                        val lit = i == vm.actionsTab || held
                        Box(Modifier.width(54.dp).fillMaxSize().clickable(touch, indication = null) { vm.actionsTab = i }) {
                            Icon(
                                tab.icon,
                                contentDescription = null,
                                tint = if (lit) Pc.Accent else Pc.TabIdle,
                                // a 50 frame centred 24 down
                                modifier = Modifier.align(Alignment.TopCenter).offset(y = (-1).dp).requiredSize(50.dp),
                            )
                            Text(
                                tab.label,
                                color = if (lit) Pc.Text else Pc.TabIdle,
                                fontSize = 12.sp,
                                fontWeight = FontWeight.Medium,
                                modifier = Modifier.align(Alignment.TopCenter).padding(top = 41.dp),
                            )
                        }
                        if (i < tabs.size - 1) Spacer(Modifier.width(10.dp))
                    }
                }
                Column(
                    Modifier.heightIn(max = 760.dp).verticalScroll(rememberScrollState()).padding(start = 12.dp, end = 12.dp, top = 20.dp, bottom = 11.dp)
                        .graphicsLayer { alpha = rowsShown.value },
                ) {
                    groups.forEachIndexed { g, all ->
                        if (g > 0) Spacer(Modifier.height(20.dp))
                        // a group led by a heading shows it above the group, not as a row of it
                        val heading = all.firstOrNull() as? Heading
                        if (heading != null) {
                            Box(Modifier.fillMaxWidth().height(45.dp)) {
                                Text(heading.label, color = Pc.PanelTitle, fontSize = 14.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 13.dp, top = 7.dp))
                            }
                        }
                        val rows = if (heading != null) all.drop(1) else all
                        Box {
                            Group(rows)
                            // beside Brush scaling, the third row
                            if (rows.any { it is Act && it.label == "Brush scaling" }) MenuPresence(scalingMenu, androidx.compose.ui.graphics.TransformOrigin(0f, 0f)) { look ->
                                ChoiceMenu(
                                    look,
                                    listOf("Relative to canvas", "Relative to screen"), if (vm.brushOnScreen) 1 else 0, offsetY = 82.dp,
                                    onPick = {
                                        vm.changeBrushScaling(it == 1)
                                        scalingMenu = false
                                    },
                                    onDismiss = { scalingMenu = false },
                                )
                            }
                        }
                    }
                    // what plugins put in the Canvas tab
                    if (shownTab == 1 && !model) PluginCanvasRows(vm)
                }
            }
        }
    }
    // the time-lapse in full or cut to 30 seconds (a tap outside: neither)
    Alert(
        if (exportingVideo) Unit else null,
        title = { "Video length" },
        message = { "" },
        action = "30 Seconds",
        onCancel = { exportingVideo = false },
        onAction = { shareVideo(30) },
        icon = PcIcons.Clock,
        destructive = false,
        cancel = "Full length",
        first = { shareVideo(null) },
    )
    // recording turned off with something recorded: deleted, or kept (a tap beside it keeps it)
    Alert(
        if (vm.purgeAsking) Unit else null,
        title = { "Purge Video?" },
        message = { "The video recorded so far will be deleted.\nIt can't be brought back." },
        action = "Purge",
        onCancel = { vm.answerPurge(false) },
        onAction = { vm.answerPurge(true) },
        icon = PcIcons.AlertQuestion,
        cancel = "Don't Purge",
    )
    PdfQualityAlert(pdfAsking != null, onCancel = { pdfAsking = null }) { quality ->
        pdfAsking?.let { share(it, quality.jpeg) }
        pdfAsking = null
    }
    dialog?.let { (title, text) ->
        AlertDialog(
            onDismissRequest = { dialog = null },
            title = { Text(title) },
            text = { Text(text, modifier = Modifier.heightIn(max = 420.dp).verticalScroll(rememberScrollState())) },
            confirmButton = { TextButton(onClick = { dialog = null }) { Text("Done") } },
        )
    }
}

/**
 * The first four tabs' rows on a model: adding without text, the canvas's reference and
 * information, sharing the model, pictures of it and its textures, and the 3D tab (the 2D
 * texture, painting through, lighting, AR). Prefs and Help are as on a canvas.
 */
private fun modelGroups(
    vm: CanvasViewModel,
    tab: Int,
    share: (CanvasViewModel.Export, Int) -> Unit,
    pickFile: () -> Unit,
    pickPhoto: () -> Unit,
    takePhoto: () -> Unit,
): List<List<ActionRow>> = when (tab) {
    0 -> listOf(
        listOf(Act("Insert a file") { pickFile() }, Act("Insert a photo") { pickPhoto() }, Act("Take a photo") { takePhoto() }),
        listOf(Act("Cut") { vm.cutLayer() }, Act("Copy") { vm.copyLayer() }, Act("Copy canvas") { vm.copyCanvas() }, Act("Paste", onClick = if (vm.canPaste) ({ vm.paste() }) else null)),
    )
    1 -> listOf(
        listOf(Toggle("Reference", vm.referenceOn) { vm.changeReferenceOn(it) }),
        listOf(Act("Canvas information") { vm.canvasInfoOpen = true }),
    )
    2 -> listOf(
        listOf(
            Heading("Share Model"),
            // ours comes back whole in another copy of the app; the other package holds no model yet
            Act(CanvasViewModel.OWN_LABEL) { share(CanvasViewModel.Export.MODEL_ARTWORK, 100) },
            Act(CanvasViewModel.PACKAGE_LABEL),
            Act("USDZ") { share(CanvasViewModel.Export.MODEL_USDZ, 100) },
            Act("OBJ") { share(CanvasViewModel.Export.MODEL_OBJ, 100) },
        ),
        listOf(
            Heading("Share Image"),
            Act("JPEG") { share(CanvasViewModel.Export.MODEL_JPEG, 100) },
            Act("PNG") { share(CanvasViewModel.Export.MODEL_RENDER, 100) },
            Act("TIFF") { share(CanvasViewModel.Export.MODEL_TIFF, 100) },
            Act("Animated GIF") { vm.modelAnimatedExport = CanvasViewModel.Animated.GIF },
            Act("Animated PNG") { vm.modelAnimatedExport = CanvasViewModel.Animated.PNG },
            Act("Animated MP4") { vm.modelAnimatedExport = CanvasViewModel.Animated.MP4 },
            Act("Animated HEVC") { vm.modelAnimatedExport = CanvasViewModel.Animated.HEVC },
        ),
        listOf(Heading("Share Textures"), Act("PNG") { share(CanvasViewModel.Export.MODEL_TEXTURES, 100) }),
    )
    3 -> {
        // showing the 2D texture, the rest is for the model
        val shown = !vm.modelFlat
        listOf(
            listOf(
                Toggle("Show 2D texture", vm.modelFlat) { vm.changeModelFlat(it) },
                Toggle("Paint through mesh", vm.paintThrough, onChange = if (shown) ({ vm.changePaintThrough(it) }) else null),
                Act("Edit lighting & environment", onClick = if (shown) ({ vm.openLightingStudio() }) else null),
            ),
            // no AR on Android yet
            listOf(Act("View in AR")),
        )
    }
    else -> emptyList()
}

/**
 * A small menu over a row's right end: its choices, each with a round mark, the chosen one ticked;
 * 254 wide, its top 7 above the row.
 */
@Composable
private fun ChoiceMenu(look: Modifier, choices: List<String>, chosen: Int, offsetY: Dp, onPick: (Int) -> Unit, onDismiss: () -> Unit) {
    val density = androidx.compose.ui.platform.LocalDensity.current
    PcPopup(
        textScale = MATCHED_POPUP_TEXT,
        alignment = Alignment.TopEnd,
        // its left edge 54 in from the row's right end
        offset = with(density) { androidx.compose.ui.unit.IntOffset(200.dp.roundToPx(), offsetY.roundToPx()) },
        onDismissRequest = onDismiss,
        properties = androidx.compose.ui.window.PopupProperties(focusable = true),
    ) {
        val shape = RoundedCornerShape(26.dp)
        Column(look.width(254.dp).clip(shape).background(Pc.MenuFill).border(1.dp, ChoiceMenuRim, shape).padding(vertical = 8.dp)) {
            choices.forEachIndexed { i, label ->
                Row(
                    Modifier.fillMaxWidth().height(40.dp).clickable { onPick(i) }.padding(start = 20.dp, end = 19.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(label, color = Pc.Text, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
                    Radio(i == chosen)
                }
            }
        }
    }
}

/** The choice menu's light rim. */
private val ChoiceMenuRim = Color(0x33FFFFFF)

/**
 * Advanced cursor settings, a page of the panel: when the brush cursor shows, and how it is drawn.
 */
@Composable
private fun CursorPage(vm: CanvasViewModel, onBack: () -> Unit) {
    BackHandler(onBack = onBack)
    Row(Modifier.padding(start = 8.dp, top = 12.dp).height(44.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(44.dp, 34.dp).clickable(onClick = onBack), contentAlignment = Alignment.Center) {
            Icon(PcIcons.ChevronLeft, contentDescription = "Back", tint = Pc.PanelTitle, modifier = Modifier.size(22.dp))
        }
        Text("Advanced cursor", color = Pc.PanelTitle, fontSize = 22.sp, fontWeight = FontWeight.Bold, maxLines = 1)
    }
    Column(Modifier.padding(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 13.dp)) {
        Text("Brush cursor visibility", color = CursorHeading, fontSize = 17.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 14.dp, top = 6.dp, bottom = 8.dp))
        ChoiceGroup(listOf("Show while hovering", "Show while painting", "Show both"), vm.cursorShows.ordinal) {
            vm.changeCursorShows(CanvasViewModel.CursorShows.entries[it])
        }
        Text("Brush outline style", color = CursorHeading, fontSize = 17.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 14.dp, top = 28.dp, bottom = 8.dp))
        ChoiceGroup(listOf("High contrast", "Active color", "Per brush"), vm.cursorStyle.ordinal) {
            vm.changeCursorStyle(CanvasViewModel.CursorStyle.entries[it])
        }
    }
}

/** What a pen button set to [a] says it does: its QuickMenu action, or (null) what Gesture controls say. */
private fun penButtonLabel(a: QuickAction?): String = a?.label ?: "Gesture controls"

/**
 * Pen buttons, a page of the panel: what the pen's first and second side buttons do, a QuickMenu
 * action each (Undo, Redo, ...) or, as they start, what Gesture controls say. A row tapped lists
 * the choices, the one set blue; a choice tapped sets it and comes back.
 */
@Composable
private fun PenButtonsPage(vm: CanvasViewModel, onBack: () -> Unit) {
    // the button whose choices are listed (true: the second), or none
    var picking by remember { mutableStateOf<Boolean?>(null) }
    val second = picking
    BackHandler { if (second != null) picking = null else onBack() }
    val title = when (second) {
        null -> "Pen buttons"
        false -> "Pen button"
        true -> "Pen second button"
    }
    Row(Modifier.padding(start = 8.dp, top = 12.dp).height(44.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(44.dp, 34.dp).clickable { if (second != null) picking = null else onBack() }, contentAlignment = Alignment.Center) {
            Icon(PcIcons.ChevronLeft, contentDescription = "Back", tint = Pc.PanelTitle, modifier = Modifier.size(22.dp))
        }
        Text(title, color = Pc.PanelTitle, fontSize = 22.sp, fontWeight = FontWeight.Bold, maxLines = 1)
    }
    // the plugins' actions a button can run, of those on: after the app's own, by their labels
    val pluginActions = remember(vm.pluginList) { vm.plugins.actionsIn(io.github.artcraftandroid.procraft.core.PluginPlace.PEN) }
    val pluginSet = { i: Int -> vm.penButtonPlugins.getOrNull(i)?.let { key -> pluginActions.firstOrNull { it.key == key } } }
    val label = { i: Int -> pluginSet(i)?.label ?: penButtonLabel(vm.penButtonActions.getOrNull(i)) }
    if (second == null) {
        Column(Modifier.padding(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 13.dp)) {
            Group(
                listOf(
                    Act("Pen button", value = label(0)) { picking = false },
                    Act("Pen second button", value = label(1)) { picking = true },
                ),
            )
        }
        return
    }
    val choices = remember { io.github.artcraftandroid.procraft.core.penButtonChoices() }
    val at = if (second) 1 else 0
    val set = vm.penButtonActions.getOrNull(at)
    val chosen = pluginSet(at)?.let { choices.size + pluginActions.indexOf(it) } ?: choices.indexOf(set).coerceAtLeast(0)
    Column(Modifier.heightIn(max = 520.dp).verticalScroll(rememberScrollState()).padding(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 13.dp)) {
        ChoiceGroup(choices.map(::penButtonLabel) + pluginActions.map { it.label }, chosen) {
            val plugin = pluginActions.getOrNull(it - choices.size)
            if (plugin != null) vm.setPenButtonPlugin(second, plugin.key) else vm.setPenButtonAction(second, choices.getOrNull(it))
            picking = null
        }
    }
}

/**
 * The plugins, a page of the panel: a card each, drawn from what the list says of them, in the
 * order they were put in. A card held by its name lifts and follows the finger or pen, faint where
 * it was; let go over another card, it takes that one's place. Remove asks first.
 */
@Composable
private fun PluginsPage(vm: CanvasViewModel, onBack: () -> Unit) {
    BackHandler(onBack = onBack)
    androidx.compose.runtime.LaunchedEffect(Unit) { vm.readPlugins() }
    val importer = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let(vm::offerPlugin)
    }
    Row(Modifier.fillMaxWidth().padding(start = 8.dp, top = 12.dp).height(44.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(44.dp, 34.dp).clickable(onClick = onBack), contentAlignment = Alignment.Center) {
            Icon(PcIcons.ChevronLeft, contentDescription = "Back", tint = Pc.PanelTitle, modifier = Modifier.size(22.dp))
        }
        Text("Plugins", color = Pc.PanelTitle, fontSize = 22.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.weight(1f))
        Box(Modifier.height(34.dp).clickable { importer.launch(arrayOf("*/*")) }.padding(horizontal = 16.dp), contentAlignment = Alignment.Center) {
            Text("Import", color = Pc.Accent, fontSize = 17.sp, maxLines = 1)
        }
    }
    val shown = vm.pluginList
    // the plugin whose Remove was tapped, while it is asked about
    var removing by remember { mutableStateOf<PluginInfo?>(null) }
    // a card held and dragged: whose, where the finger is and where on the card it took hold
    var dragged by remember { mutableStateOf<String?>(null) }
    var dragAt by remember { mutableStateOf(Offset.Zero) }
    var grab by remember { mutableStateOf(Offset.Zero) }
    // where the cards are (window pixels), for a dragged one to land, and how tall their heads are
    val cardBounds = remember { androidx.compose.runtime.mutableStateMapOf<String, androidx.compose.ui.geometry.Rect>() }
    val headHeights = remember { androidx.compose.runtime.mutableStateMapOf<String, Int>() }
    var listBounds by remember { mutableStateOf(androidx.compose.ui.geometry.Rect.Zero) }
    // let go over another card: to its place
    fun drop() {
        val id = dragged ?: return
        dragged = null
        val to = shown.indexOfFirst { cardBounds[it.id]?.contains(dragAt) == true }
        if (to >= 0 && shown.getOrNull(to)?.id != id) vm.movePlugin(id, to)
    }
    // (for a hold that began before the list last changed)
    val dropNow by rememberUpdatedState(::drop)
    Box(Modifier.onGloballyPositioned { listBounds = it.boundsInWindow() }) {
        Column(
            Modifier.verticalScroll(rememberScrollState()).padding(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 13.dp),
            verticalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(20.dp),
        ) {
            if (shown.isEmpty()) Text("Nothing imported yet. Import a .procraft-plugin file to add one.", color = Pc.ActionsRow, fontSize = 15.sp, modifier = Modifier.padding(16.dp))
            for (plugin in shown) androidx.compose.runtime.key(plugin.id) {
                val id = plugin.id
                // gone from the list, it is nowhere: where it last was may be under another card
                androidx.compose.runtime.DisposableEffect(id) {
                    onDispose {
                        cardBounds.remove(id)
                        headHeights.remove(id)
                    }
                }
                PluginCard(
                    plugin,
                    vm,
                    Modifier.onGloballyPositioned { cardBounds[id] = it.boundsInWindow() }.alpha(if (dragged == id) 0.3f else 1f),
                    head = Modifier.onSizeChanged { headHeights[id] = it.height }.pointerInput(id) {
                        // held still, the card lifts; the rest of the press is its drag's, taken before
                        // the list under it sees it (moved first, the list scrolls instead)
                        awaitEachGesture {
                            val down = awaitFirstDown(requireUnconsumed = false)
                            val long = awaitLongPressOrCancellation(down.id) ?: return@awaitEachGesture
                            grab = long.position
                            dragAt = (cardBounds[id]?.topLeft ?: Offset.Zero) + long.position
                            dragged = id
                            try {
                                while (true) {
                                    val change = awaitPointerEvent(androidx.compose.ui.input.pointer.PointerEventPass.Initial).changes.firstOrNull { it.id == long.id }
                                    if (change == null || change.isConsumed) {
                                        dragged = null
                                        break
                                    }
                                    val by = change.positionChangeIgnoreConsumed()
                                    change.consume()
                                    if (!change.pressed) {
                                        dropNow()
                                        break
                                    }
                                    dragAt += by
                                }
                            } catch (gone: kotlinx.coroutines.CancellationException) {
                                dragged = null
                                throw gone
                            }
                        }
                    },
                    onRemove = { removing = plugin },
                )
            }
        }
        // the dragged card follows the finger: its name and what it does
        dragged?.let { id ->
            val plugin = shown.firstOrNull { it.id == id } ?: return@let
            val width = cardBounds[id]?.width ?: 0f
            val density = androidx.compose.ui.platform.LocalDensity.current
            val at = dragAt - grab - listBounds.topLeft
            Box(
                Modifier.offset { androidx.compose.ui.unit.IntOffset(at.x.roundToInt(), at.y.roundToInt()) }
                    .size(with(density) { width.toDp() }, with(density) { (headHeights[id] ?: 0).toDp() })
                    .clip(RoundedCornerShape(22.dp)).background(Pc.RowSelected.copy(alpha = 0.9f)),
            ) {
                PluginHead(plugin, lifted = true)
            }
        }
    }
    Alert(
        removing,
        title = { "Remove this plugin?" },
        message = { "“${it.name}” goes, and its settings with it." },
        action = "Remove",
        icon = PcIcons.Trash,
        onCancel = { removing = null },
        onAction = {
            removing = null
            vm.removePlugin(it)
        },
    )
}

/** A plugin card's head: its name, what it does and what is the matter with it; all in white while it is [lifted] and dragged. */
@Composable
private fun PluginHead(plugin: PluginInfo, modifier: Modifier = Modifier, lifted: Boolean = false) {
    Column(modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(plugin.name, color = if (lifted) Color.White else Pc.PanelTitle, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
        }
        Text(plugin.description, color = if (lifted) Color.White else Pc.ActionsRow, fontSize = 14.sp, modifier = Modifier.padding(top = 6.dp))
        plugin.problem?.let { Text(it, color = if (lifted) Color.White else Pc.SlideValue, fontSize = 14.sp, fontWeight = FontWeight.Medium, modifier = Modifier.padding(top = 4.dp)) }
    }
}

/**
 * A plugin: its head ([head] where it is held to drag it), its controls whatever they are, what it
 * does when tapped, and the rows that turn it on or off and remove it ([onRemove], which asks).
 */
@Composable
private fun PluginCard(plugin: PluginInfo, vm: CanvasViewModel, modifier: Modifier, head: Modifier, onRemove: () -> Unit) {
    Column(modifier.fillMaxWidth().clip(RoundedCornerShape(22.dp)).background(Pc.ActionsGroup)) {
        PluginHead(plugin, modifier = head)
        if (plugin.on && plugin.problem == null && plugin.features.isNotEmpty()) {
            // made of features: each a heading with its switch, what comes under it while it is on
            for (feature in plugin.features) {
                Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
                PluginFeatureHead(feature, vm)
                if (!feature.control.on) continue
                for (control in feature.controls) {
                    Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
                    PluginControlRow(control, vm)
                }
                for (action in feature.actions) {
                    Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
                    PluginActionRow(action.label) { vm.runPluginAction(action.key) }
                }
            }
        } else if (plugin.on && plugin.problem == null) {
            for (control in plugin.controls) {
                Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
                PluginControlRow(control, vm)
            }
            for (action in plugin.actions) {
                Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
                PluginActionRow(action.label) { vm.runPluginAction(action.key) }
            }
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
        ActionRowView(Act(if (plugin.on) "Turn off" else "Turn on") { vm.setPluginOn(plugin, !plugin.on) })
        if (plugin.source == PluginSource.FILE) {
            Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
            ActionRowView(Act("Remove", onClick = onRemove))
        }
    }
}

/** One of a plugin's features as its card shows it: its name and words, and the switch that turns it on. */
@Composable
private fun PluginFeatureHead(feature: io.github.artcraftandroid.procraft.core.PluginFeature, vm: CanvasViewModel) {
    val switch = feature.control
    Row(
        Modifier.fillMaxWidth().clickable { vm.setPluginSwitch(switch.key, !switch.on) }.padding(start = 16.dp, end = 8.dp, top = 10.dp, bottom = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f).padding(end = 8.dp)) {
            Text(switch.label, color = Pc.PanelTitle, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
            if (feature.description.isNotEmpty()) Text(feature.description, color = Pc.ActionsRow, fontSize = 14.sp, modifier = Modifier.padding(top = 4.dp))
        }
        Switch(switch.on, true)
    }
}

/** A plugin's control as its card shows it, whatever it is: a switch, a slider, a setting named at every step, a file. */
@Composable
private fun PluginControlRow(control: PluginControl, vm: CanvasViewModel) {
    when (control) {
        is PluginControl.Switch -> ActionRowView(Toggle(control.control.label, control.control.on) { vm.setPluginSwitch(control.control.key, it) })
        is PluginControl.Slider -> {
            val choices = pluginSliderChoices(control.control)
            if (choices.isEmpty()) PluginSliderRow(control.control, vm::setPluginSlider)
            else PluginChoiceRow(control.control, choices, vm::setPluginSlider)
        }
        is PluginControl.File -> PluginFileRow(control.control, vm)
        is PluginControl.Texture -> PluginTextureRow(control.control, vm)
    }
}

/**
 * The controls and actions the plugins that are on put in the Canvas tab, after its own rows: a
 * group of their own, each row as on its plugin's card.
 */
@Composable
private fun PluginCanvasRows(vm: CanvasViewModel) {
    val list = vm.pluginList
    val controls = remember(list) { vm.plugins.controlsIn(io.github.artcraftandroid.procraft.core.PluginPlace.CANVAS) }
    val actions = remember(list) { vm.plugins.actionsIn(io.github.artcraftandroid.procraft.core.PluginPlace.CANVAS) }
    // Edit Layout, while a plugin lets the bars move
    val edits = vm.layoutLook.edit
    if (controls.isEmpty() && actions.isEmpty() && !edits) return
    Spacer(Modifier.height(20.dp))
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(22.dp)).background(Pc.ActionsGroup)) {
        controls.forEachIndexed { i, control ->
            if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
            PluginControlRow(control, vm)
        }
        actions.forEachIndexed { i, action ->
            if (i > 0 || controls.isNotEmpty()) Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
            PluginActionRow(action.label) { vm.runPluginAction(action.key) }
        }
        if (edits) {
            if (controls.isNotEmpty() || actions.isNotEmpty()) Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
            PluginActionRow("Edit Layout") { vm.startEditLayout() }
        }
    }
}

/**
 * A plugin's own panel, while its action keeps it open: floating under the bar at the right, its
 * title and Done, then its rows on a card as the Actions panel's are. A slider is told as it lets go,
 * anything else at once.
 */
@Composable
fun PluginPanelView(vm: CanvasViewModel, modifier: Modifier = Modifier) {
    val panel = vm.pluginPanel ?: return
    Popover(340.dp, 0.dp, modifier, side = ArrowSide.NONE) {
        Row(Modifier.fillMaxWidth().height(52.dp).padding(start = 24.dp, end = 24.dp, top = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(panel.title, color = Pc.PanelTitle, fontSize = 22.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.weight(1f))
            Text("Done", color = Pc.Accent, fontSize = 17.sp, maxLines = 1, modifier = Modifier.clickable { vm.closePluginPanel() })
        }
        Column(Modifier.verticalScroll(rememberScrollState()).padding(start = 10.dp, top = 8.dp, end = 10.dp, bottom = 10.dp)) {
            Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(13.dp)).background(Pc.ActionsGroup)) {
                panel.rows.forEachIndexed { i, row ->
                    if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
                    when (row) {
                        is PanelRow.Text -> Text(row.text, color = Pc.ActionsRow, fontSize = 15.sp, modifier = Modifier.padding(horizontal = 16.dp, vertical = 11.dp))
                        is PanelRow.Slider -> PanelSliderRow(row) { vm.touchPluginPanel(i, it) }
                        is PanelRow.Switch -> ActionRowView(Toggle(row.label, row.on) { vm.touchPluginPanel(i, if (it) 1f else 0f) })
                        is PanelRow.Choice -> PanelChoiceRow(row) { vm.touchPluginPanel(i, it.toFloat()) }
                        is PanelRow.Button -> PluginActionRow(row.label) { vm.touchPluginPanel(i, 1f) }
                        is PanelRow.Colours -> PanelColoursRow(row) { vm.touchPluginPanel(i, it.toFloat()) }
                        is PanelRow.Keys -> PanelKeysRow(row) { vm.touchPluginPanel(i, 1f) }
                        is PanelRow.Brushes -> PanelBrushesRow(vm, row) { vm.touchPluginPanel(i, it.toFloat()) }
                        // one of its texture controls, as its card shows it (others show nothing)
                        is PanelRow.Control -> vm.pluginList.flatMap { it.controls }.filterIsInstance<PluginControl.Texture>()
                            .firstOrNull { it.control.key == row.key }?.let { PluginTextureRow(it.control, vm) }
                    }
                }
            }
        }
    }
}

/** A slider on a plugin's panel: its value whole with whole steps, else to two places, then its unit. */
@Composable
private fun PanelSliderRow(row: PanelRow.Slider, onSet: (Float) -> Unit) {
    var moved by remember(row) { mutableStateOf<Float?>(null) }
    val value = moved ?: row.value
    val whole = row.step >= 1f && row.step % 1f == 0f
    val shown = (if (whole) value.roundToInt().toString() else String.format(java.util.Locale.ROOT, "%.2f", value)) + row.unit
    SlideRow(
        Slide(
            row.label,
            shown,
            (value - row.least) / (row.most - row.least),
            onDone = {
                moved?.let(onSet)
                moved = null
            },
            onChange = { along ->
                val to = row.least + along * (row.most - row.least)
                val stepped = if (row.step > 0f) row.least + ((to - row.least) / row.step).roundToInt() * row.step else to
                moved = stepped.coerceIn(row.least, row.most)
            },
        ),
    )
}

/** A choice on a plugin's panel: its label, the one picked in blue; a tap picks the next. */
@Composable
private fun PanelChoiceRow(row: PanelRow.Choice, onPick: (Int) -> Unit) {
    val count = row.options.size.coerceAtLeast(1)
    Row(
        Modifier.fillMaxWidth().height(44.dp).clickable { onPick((row.chosen.toInt() + 1) % count) }.padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(row.label, color = Pc.ActionsRow, fontSize = 17.sp, maxLines = 1)
        Box(Modifier.weight(1f))
        Text(row.options.getOrElse(row.chosen.toInt()) { "" }, color = Pc.Accent, fontSize = 17.sp, maxLines = 1)
    }
}

/** A row of colours on a plugin's panel: its label, then each colour a disc, the one chosen ringed; a tap picks one. */
@Composable
private fun PanelColoursRow(row: PanelRow.Colours, onPick: (Int) -> Unit) {
    Row(Modifier.fillMaxWidth().height(52.dp).padding(start = 16.dp, end = 10.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(row.label, color = Pc.ActionsRow, fontSize = 17.sp, maxLines = 1, modifier = Modifier.padding(end = 8.dp))
        Box(Modifier.weight(1f))
        row.colours.forEachIndexed { at, colour ->
            val chosen = row.chosen?.toInt() == at
            Box(Modifier.size(30.dp).clip(CircleShape).clickable { onPick(at) }, contentAlignment = Alignment.Center) {
                Box(
                    Modifier.size(24.dp).clip(CircleShape)
                        .then(if (chosen) Modifier.border(2.dp, Pc.ActionsRow, CircleShape) else Modifier)
                        .padding(if (chosen) 4.dp else 0.dp).clip(CircleShape)
                        .background(Color(0xFF000000L or colour.toLong())),
                )
            }
        }
    }
}

/** A row of brushes on a plugin's panel: its label, then each brush's sample stroke in a disc, the one chosen ringed; a tap picks one. */
@Composable
private fun PanelBrushesRow(vm: CanvasViewModel, row: PanelRow.Brushes, onPick: (Int) -> Unit) {
    Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 10.dp, top = 10.dp, bottom = 10.dp)) {
        Text(row.label, color = Pc.ActionsRow, fontSize = 17.sp, maxLines = 1)
        Row(Modifier.padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            row.brushes.forEachIndexed { at, b ->
                RingBrush(vm, b, row.chosen?.toInt() == at, 34.dp, Modifier) { onPick(at) }
            }
        }
    }
}

/** A row of keys on a plugin's panel: its label, the keys at its end as a file control's name is; tapped as a button is. */
@Composable
private fun PanelKeysRow(row: PanelRow.Keys, onClick: () -> Unit) {
    androidx.compose.ui.layout.Layout(
        content = {
            Text(row.label, color = Pc.ActionsRow, fontSize = 17.sp, maxLines = 1, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis)
            Text(row.keys, color = Pc.SlideValue, fontSize = 15.sp, maxLines = 1)
        },
        modifier = Modifier.fillMaxWidth().height(44.dp).clickable(onClick = onClick).padding(horizontal = 16.dp),
    ) { measurables, constraints ->
        val loose = constraints.copy(minWidth = 0, minHeight = 0)
        val keys = measurables[1].measure(loose)
        val (labelMost, keysAt) = keysRowPlaces(constraints.maxWidth, keys.width, 12.dp.roundToPx())
        val label = measurables[0].measure(loose.copy(maxWidth = labelMost))
        val height = constraints.maxHeight
        layout(constraints.maxWidth, height) {
            label.placeRelative(0, (height - label.height) / 2)
            keys.placeRelative(keysAt, (height - keys.height) / 2)
        }
    }
}

/**
 * Where a row of keys [width] px wide puts its parts: the keys ([keys] px) end where it ends, and
 * its label has all the room before them but [gap] (none if the keys take it all), cut short only
 * past that. The label's most width, and where the keys begin.
 */
internal fun keysRowPlaces(width: Int, keys: Int, gap: Int): Pair<Int, Int> {
    val keysAt = (width - keys).coerceAtLeast(0)
    return (keysAt - gap).coerceAtLeast(0) to keysAt
}

/** One of a plugin's actions on its card: its words in blue, run when tapped. */
@Composable
private fun PluginActionRow(label: String, onClick: () -> Unit) {
    Box(Modifier.fillMaxWidth().height(44.dp).clickable(onClick = onClick).padding(horizontal = 16.dp), contentAlignment = Alignment.CenterStart) {
        Text(label, color = Pc.Accent, fontSize = 17.sp, maxLines = 1)
    }
}

/** A file control on a card (a sound or a picture): its label, then the name of what was picked and Clear, or Import; a tap on the row picks another. */
@Composable
private fun PluginFileRow(file: PluginFile, vm: CanvasViewModel) {
    val picture = file.accepts == io.github.artcraftandroid.procraft.core.PluginFileKind.PICTURE
    val picker = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { if (picture) vm.pickPluginPicture(file.key, it) else vm.pickPluginSound(file.key, it) }
    }
    Row(
        Modifier.fillMaxWidth().height(44.dp).clickable { picker.launch(arrayOf(if (picture) "image/*" else "audio/*")) }.padding(start = 16.dp, end = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(file.label, color = Pc.ActionsRow, fontSize = 17.sp, maxLines = 1)
        Box(Modifier.weight(1f))
        val chosen = file.chosen
        if (chosen != null) {
            Text(chosen, color = Pc.SlideValue, fontSize = 15.sp, maxLines = 1, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis, modifier = Modifier.padding(start = 12.dp).weight(1f, fill = false))
            Text("Clear", color = Pc.Accent, fontSize = 17.sp, maxLines = 1, modifier = Modifier.clickable { vm.clearPluginFile(file.key) }.padding(start = 12.dp))
        } else {
            Text("Import", color = Pc.Accent, fontSize = 17.sp, maxLines = 1)
        }
    }
}

/** A setting on a card named at every step: its label, the one chosen in blue; a tap opens a menu of them. */
@Composable
private fun PluginChoiceRow(slider: PluginSlider, choices: List<String>, onSet: (PluginSlider, Float) -> Unit) {
    var open by remember(slider.key) { mutableStateOf(false) }
    Box {
        Row(
            Modifier.fillMaxWidth().height(44.dp).clickable { open = true }.padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(slider.label, color = Pc.ActionsRow, fontSize = 17.sp, maxLines = 1)
            Box(Modifier.weight(1f))
            Text(pluginSliderShown(slider, slider.value), color = Pc.Accent, fontSize = 17.sp, maxLines = 1)
        }
        MenuPresence(open, androidx.compose.ui.graphics.TransformOrigin(0f, 0f)) { look ->
            ChoiceMenu(
                look,
                choices, pluginSliderChosen(slider, slider.value).toInt(), offsetY = (-7).dp,
                onPick = {
                    onSet(slider, pluginSliderChoice(slider, it.toUInt()))
                    open = false
                },
                onDismiss = { open = false },
            )
        }
    }
}

/** A slider on a card: the row follows the finger, and the value goes in as it lets go, as what it sets may move the row itself, unless it is live. */
@Composable
private fun PluginSliderRow(slider: PluginSlider, onSet: (PluginSlider, Float) -> Unit) {
    var moved by remember(slider.key) { mutableStateOf<Float?>(null) }
    val value = moved ?: slider.value
    SlideRow(
        Slide(
            slider.label,
            pluginSliderShown(slider, value),
            pluginSliderAlong(slider, value),
            onDone = {
                moved?.let { onSet(slider, it) }
                moved = null
            },
            onChange = {
                val to = pluginSliderAt(slider, it)
                if (slider.live) onSet(slider, to) else moved = to
            },
        ),
    )
}

/** The one screen that asks whether to import a plugin file: what it is, and what it can do. Also where a setting that has moved to a plugin is said, once. */
@Composable
fun PluginOfferAlert(vm: CanvasViewModel) {
    Alert(
        vm.movedNotice,
        title = { "Now a plugin" },
        message = { it },
        action = "OK",
        onCancel = vm::movedNoticeRead,
        onAction = { vm.movedNoticeRead() },
        destructive = false,
        alone = true,
    )
    Alert(
        vm.pluginOffer,
        title = { it.name },
        message = { it.message },
        action = "Import",
        onCancel = vm::declinePlugin,
        onAction = { vm.acceptPlugin() },
        destructive = false,
    )
}

/** The 32-bit build on a tablet that runs the 64-bit one: which to install instead, once a version. */
@Composable
fun WrongBuildAlert(vm: CanvasViewModel) {
    val context = androidx.compose.ui.platform.LocalContext.current
    Alert(
        Unit.takeIf { vm.wrongBuildNotice },
        title = { "Use the 64-bit version" },
        message = {
            "This is HanaCreate's 32-bit version, for older tablets. This tablet runs the 64-bit one, which newer " +
                "Android expects: install the file ending in arm64.apk over this one, and your artworks stay."
        },
        action = "Releases",
        cancel = "Later",
        onCancel = vm::wrongBuildNoticeRead,
        onAction = {
            vm.wrongBuildNoticeRead()
            runCatching {
                context.startActivity(
                    android.content.Intent(android.content.Intent.ACTION_VIEW, android.net.Uri.parse(RELEASES_PAGE))
                        .addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK),
                )
            }
        },
        destructive = false,
    )
}

/** Where each release's files are. */
private const val RELEASES_PAGE = "https://github.com/artcraft-android/procraft/releases/latest"

/** Choices as rows of a rounded group, the chosen one a full blue row. */
@Composable
private fun ChoiceGroup(choices: List<String>, chosen: Int, onPick: (Int) -> Unit) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(Pc.ActionsGroup)) {
        choices.forEachIndexed { i, label ->
            if (i > 0 && i != chosen && i - 1 != chosen) Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
            Box(
                Modifier.fillMaxWidth().height(42.dp).background(if (i == chosen) Pc.Accent else Color.Transparent).clickable { onPick(i) }.padding(start = 16.dp),
                contentAlignment = Alignment.CenterStart,
            ) {
                Text(label, color = if (i == chosen) Color.White else Pc.PanelTitle, fontSize = 17.sp, maxLines = 1)
            }
        }
    }
}

/** Rows in a rounded group, a line between them (not between sliders). */
@Composable
private fun Group(rows: List<ActionRow>) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(22.dp)).background(Pc.ActionsGroup)) {
        rows.forEachIndexed { i, row ->
            if (i > 0 && !(row is Slide && rows[i - 1] is Slide)) Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
            if (row is Slide) SlideRow(row) else ActionRowView(row)
        }
    }
}

/**
 * Pressure and Smoothing, a page of the panel: the app's stabilization, motion filtering and its
 * expression, Tip Attachment, the pressure curve every stroke goes through, and Reset all.
 */
@Composable
private fun PressurePage(vm: CanvasViewModel, onBack: () -> Unit) {
    BackHandler(onBack = onBack)
    Row(Modifier.padding(start = 8.dp, top = 12.dp).height(44.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(44.dp, 34.dp).clickable(onClick = onBack), contentAlignment = Alignment.Center) {
            Icon(PcIcons.ChevronLeft, contentDescription = "Back", tint = Pc.PanelTitle, modifier = Modifier.size(22.dp))
        }
        Text("Pressure & Smoothing", color = Pc.PanelTitle, fontSize = 22.sp, fontWeight = FontWeight.Bold, maxLines = 1)
    }
    Column(Modifier.heightIn(max = 760.dp).verticalScroll(rememberScrollState()).padding(start = 12.dp, end = 12.dp, top = 20.dp, bottom = 13.dp)) {
        Group(
            listOf(
                Slide("Stabilization", percent(vm.inputStabilization), vm.inputStabilization, 64.dp) { vm.changeInput(stabilization = it) },
                Slide("Motion Filtering", percent(vm.inputMotionFiltering), vm.inputMotionFiltering, 64.dp) { vm.changeInput(motion = it) },
                Slide("Motion Filtering Expression", percent(vm.inputMotionExpression), vm.inputMotionExpression, 64.dp) { vm.changeInput(expression = it) },
            ),
        )
        Spacer(Modifier.height(16.dp))
        // with no filtering it has nothing to do
        val tipChange: ((Boolean) -> Unit)? = if (vm.inputMotionFiltering > 0f) ({ on -> vm.changeInput(tipAttached = on) }) else null
        Group(listOf(Toggle("Tip Attachment", vm.inputTipAttached, onChange = tipChange)))
        Spacer(Modifier.height(24.dp))
        Text("App Pressure Sensitivity", color = Pc.SlideLabel, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 13.dp).height(19.dp))
        Box(Modifier.padding(horizontal = 7.dp).fillMaxWidth().aspectRatio(1f).background(CurveFill)) {
            PressureCurve(vm.inputCurve, Modifier.fillMaxSize(), grid = CurveGrid, edge = CurveEdge, line = CurveLine) { vm.changeInput(curve = it) }
        }
        Spacer(Modifier.height(27.dp))
        Group(listOf(Act("Reset all") { vm.resetInput() }))
    }
}

/** A slider row: the name and value on top, the bar under them; drag or tap along it. */
@Composable
private fun SlideRow(row: Slide) {
    val change by rememberUpdatedState(row.onChange)
    val done by rememberUpdatedState(row.onDone)
    Box(
        Modifier.fillMaxWidth().height(row.tall)
            .pointerInput(Unit) {
                val inset = 16.dp.toPx()
                fun at(x: Float) = ((x - inset) / (size.width - 2f * inset)).coerceIn(0f, 1f)
                detectTapGestures {
                    change(at(it.x))
                    done()
                }
            }
            .pointerInput(Unit) {
                val inset = 16.dp.toPx()
                fun at(x: Float) = ((x - inset) / (size.width - 2f * inset)).coerceIn(0f, 1f)
                detectHorizontalDragGestures(onDragStart = { change(at(it.x)) }, onDragEnd = { done() }, onDragCancel = { done() }) { c, _ ->
                    c.consume()
                    change(at(c.position.x))
                }
            }
            .drawBehind {
                val inset = 16.dp.toPx()
                val y = size.height - 17.5.dp.toPx()
                val h = 3.dp.toPx()
                val x = inset + row.fraction.coerceIn(0f, 1f) * (size.width - 2f * inset)
                drawRect(Pc.SlideTrack, Offset(inset, y - h / 2f), Size(size.width - 2f * inset, h))
                drawRect(Pc.Accent, Offset(inset, y - h / 2f), Size(x - inset, h))
                val knob = Size(16.dp.toPx(), 10.dp.toPx())
                drawRoundRect(Color.White, Offset(x - knob.width / 2f, y - knob.height / 2f), knob, CornerRadius(knob.height / 2f))
            },
    ) {
        Text(row.label, color = Pc.SlideLabel, fontSize = 17.sp, maxLines = 1, modifier = Modifier.padding(start = 16.dp, top = 9.dp))
        Text(row.value, color = Pc.SlideValue, fontSize = 17.sp, maxLines = 1, modifier = Modifier.align(Alignment.TopEnd).padding(end = 19.dp, top = 9.dp))
    }
}

/** How far a row with a private action slides open, that action's button and its look. */
private val PrivateSlide = 248.dp
private val PrivateWidth = 245.dp
private val PrivateFill = Color(0xFF454545)
private val PrivateInk = Color(0xFFA8A8A8)

/**
 * A row that a swipe to the left slides open, its private action's button coming in from the
 * right; let go past halfway it stays open. A tap on the row closes it again.
 */
@Composable
private fun PrivateSwipe(row: Act, content: @Composable () -> Unit) {
    val scope = rememberCoroutineScope()
    val slide = remember { androidx.compose.animation.core.Animatable(0f) }
    val full = with(androidx.compose.ui.platform.LocalDensity.current) { PrivateSlide.toPx() }
    fun settle(open: Boolean) {
        scope.launch { slide.animateTo(if (open) full else 0f, Motion.swipeSettle()) }
    }
    Box(
        Modifier.fillMaxWidth().height(44.dp).clipToBounds()
            .pointerInput(Unit) {
                detectHorizontalDragGestures(
                    onDragEnd = { settle(slide.value > full / 2f) },
                    onDragCancel = { settle(false) },
                ) { c, dx ->
                    c.consume()
                    scope.launch { slide.snapTo((slide.value - dx).coerceIn(0f, full)) }
                }
            },
    ) {
        Box(Modifier.fillMaxSize().graphicsLayer { translationX = -slide.value }) { content() }
        if (slide.value > 0f) {
            Box(Modifier.matchParentSize().clickable(remember { androidx.compose.foundation.interaction.MutableInteractionSource() }, indication = null) { settle(false) })
            Box(
                Modifier.align(Alignment.CenterEnd).width(PrivateWidth).fillMaxHeight()
                    .graphicsLayer { translationX = full - slide.value }
                    .clip(RoundedCornerShape(13.dp)).background(PrivateFill)
                    .then(
                        row.onPrivate?.let { act ->
                            Modifier.clickable {
                                scope.launch { slide.snapTo(0f) }
                                act()
                            }
                        } ?: Modifier,
                    ),
                contentAlignment = Alignment.Center,
            ) {
                Text(row.private.orEmpty(), color = if (row.onPrivate != null) PrivateInk else Pc.ActionsDisabled, fontSize = 17.sp, maxLines = 1)
            }
        }
    }
}

@Composable
private fun ActionRowView(row: ActionRow) {
    if (row is Act && row.private != null) {
        PrivateSwipe(row) { ActionRowContent(row) }
    } else {
        ActionRowContent(row)
    }
}

@Composable
private fun ActionRowContent(row: ActionRow) {
    val enabled = when (row) {
        is Act -> row.onClick != null
        is Toggle -> row.onChange != null
        is Heading, is Slide -> true
    }
    // a button row turns blue while it is held, its text white
    val touch = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }
    val lit = animateHighlight(heldOf(touch))
    val ink = androidx.compose.ui.graphics.lerp(if (enabled || (row is Toggle && row.fixed)) Pc.ActionsRow else Pc.ActionsDisabled, Color.White, lit)
    Row(
        Modifier.fillMaxWidth().height(44.dp)
            .then(if (lit > 0f) Modifier.background(Pc.Accent.copy(alpha = lit)) else Modifier)
            .then(if (row is Act && row.onClick != null) Modifier.clickable(touch, indication = null, onClick = row.onClick) else Modifier)
            .then(if (row is Toggle && row.onChange != null) Modifier.clickable { row.onChange.invoke(!row.checked) } else Modifier)
            .padding(start = 16.dp, end = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        when (row) {
            is Heading -> Text(row.label, color = Pc.PanelTitle, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
            is Act -> {
                Text(row.label, color = ink, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
                row.value?.let { Text(it, color = androidx.compose.ui.graphics.lerp(Pc.TabIdle, Color.White, lit), fontSize = 15.sp, modifier = Modifier.padding(end = 8.dp)) }
            }
            is Toggle -> {
                Text(row.label, color = ink, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
                Switch(row.checked, enabled)
            }
            is Slide -> SlideRow(row)
        }
    }
}

/**
 * A 63 × 28 switch: a blue track when on, grey when off, with a white knob. Flipped, the knob swells
 * into a clear lens as it slides across, the track's colour fading under it, then settles white.
 */
@Composable
internal fun Switch(checked: Boolean, enabled: Boolean, fixedTrack: Color = Pc.SwitchFixed, fixedKnob: Color = Pc.SwitchFixedKnob, offTrack: Color = Pc.SwitchOff) {
    val on = androidx.compose.animation.core.animateFloatAsState(if (checked) 1f else 0f, Motion.switchKnob(), label = "knob").value
    val lens = remember { androidx.compose.animation.core.Animatable(0f) }
    var flipped by remember { mutableStateOf(false) }
    LaunchedEffect(checked) {
        // not when it is first shown
        if (!flipped) {
            flipped = true
            return@LaunchedEffect
        }
        lens.animateTo(1f, Motion.switchSwell())
        kotlinx.coroutines.delay(Motion.SWITCH_HOLD_MS)
        lens.animateTo(0f, Motion.switchSettle())
    }
    val g = lens.value
    val track = if (!enabled) fixedTrack else androidx.compose.ui.graphics.lerp(offTrack, Pc.Accent, on)
    // not clipped to the track: the swollen knob spills over it
    Box(Modifier.size(63.dp, 28.dp)) {
        Box(Modifier.matchParentSize().clip(CircleShape).background(track))
        Box(
            Modifier.offset(x = 2.dp + 22.dp * on, y = 2.dp).size(37.dp, 24.dp)
                .graphicsLayer {
                    scaleX = 1f + Motion.SWITCH_SWELL_X * g
                    scaleY = 1f + Motion.SWITCH_SWELL_Y * g
                }
                .clip(CircleShape)
                .background((if (enabled) Color.White else fixedKnob).copy(alpha = 1f - 0.7f * g))
                .border(1.dp, Color.White.copy(alpha = 0.55f * g), CircleShape),
        )
    }
}


private fun versionName(context: Context): String =
    runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull() ?: "?"

private fun licences(context: Context): String {
    fun asset(name: String) = runCatching { context.assets.open(name).bufferedReader().use { it.readText() } }.getOrDefault("")
    return "The interface font is Inter, by The Inter Project Authors.\n\n${asset("fonts/Inter-OFL.txt")}\n\n" +
        "The app keeps Lucide icons (lucide.dev), by Lucide Icons and Contributors.\n\n${asset("icons/LICENSE-lucide.txt")}"
}

/** Offers [files] to other apps through the share sheet. */
internal fun shareFiles(context: Context, files: List<File>, mime: String) {
    if (files.size == 1) return shareFile(context, files[0], mime)
    val uris = ArrayList(files.map { FileProvider.getUriForFile(context, context.packageName + ".files", it) })
    val send = Intent(Intent.ACTION_SEND_MULTIPLE).setType(mime).putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    context.startActivity(shareSheet(context, send, files, mime))
}

/** What is said when the log couldn't be made ready to share (the storage full, say). */
internal const val LOG_NOT_READY = "The log couldn't be made ready to share."

/**
 * The app's log made ready to share (off the main thread, in [scope]) and offered through the share
 * sheet: Help's Share log, the gallery's question about an artwork that closed the app, and the
 * app's own settings page. [failed] hears when it couldn't be made ready.
 */
internal fun shareLog(context: Context, scope: kotlinx.coroutines.CoroutineScope, failed: () -> Unit) {
    scope.launch {
        val log = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) { io.github.artcraftandroid.procraft.AppLog.toShare(context) }
        if (log == null) failed() else runCatching { shareFile(context, log, "text/plain") }
    }
}

/** Offers [file] to other apps through the share sheet. */
internal fun shareFile(context: Context, file: File, mime: String) {
    val uri = FileProvider.getUriForFile(context, context.packageName + ".files", file)
    val send = Intent(Intent.ACTION_SEND).setType(mime).putExtra(Intent.EXTRA_STREAM, uri).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    context.startActivity(shareSheet(context, send, listOf(file), mime))
}

/**
 * The share sheet for [send], with saving [files] to the device always among its choices, and to
 * the gallery too for pictures and videos.
 */
private fun shareSheet(context: Context, send: Intent, files: List<File>, mime: String): Intent {
    fun ours(intent: Intent, label: String) =
        android.content.pm.LabeledIntent(intent, context.packageName, label, io.github.artcraftandroid.procraft.R.mipmap.ic_launcher)
    val gallery = ours(io.github.artcraftandroid.procraft.SaveToGalleryActivity.intent(context, files, mime), "Save to Gallery")
        .takeIf { io.github.artcraftandroid.procraft.savesToGallery(mime) }
    val save = ours(io.github.artcraftandroid.procraft.SaveToDeviceActivity.intent(context, files, mime), "Save to device")
    val choices = listOfNotNull(gallery, save).toTypedArray<android.os.Parcelable>()
    return Intent.createChooser(send, null).putExtra(Intent.EXTRA_INITIAL_INTENTS, choices)
}
