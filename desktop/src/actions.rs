//! The Actions panel, as the Android app's (ui/ActionsPanel.kt): six tabs (Add, Canvas, Share,
//! Video, Prefs, Help; on a model, 3D in Video's place) of grouped rows, and the pages Prefs opens (Advanced cursor, Pressure and
//! Smoothing). Rows the desktop can't do yet show dimmed, as the Android app's own rows still to
//! come do. Share writes the artwork to a file.

use std::collections::HashMap;
use std::time::Instant;

use egui::{Align2, Color32, Id, Order, Painter, Pos2, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use procraft_core::{
    PanelRow, PluginAction, PluginControl, PluginInfo, PluginPanel, PluginSlider, PluginSource, Plugins,
    plugin_slider_along, plugin_slider_at, plugin_slider_choice, plugin_slider_choices, plugin_slider_chosen,
    plugin_slider_shown,
};

use crate::alert::{Alert, Answer};
use crate::animated_export::Animated;
use crate::files::Format;
use crate::icons;
use crate::look::{Look, Themed, Weight, argb, cubic_bezier, highlight, mix, pc, text, wrapped};
use crate::menu::{Glass, Menu};
use crate::panel::{self, PANEL_TITLE};
use crate::prefs::{CursorShows, CursorStyle, Prefs, RAPID_UNDO_MAX_S, RAPID_UNDO_MIN_S};

const ACTIONS_WIDTH: f32 = 390.0;
const TAB_IDLE: Themed = Themed(0xFFA3A3A3, 0xFF404040);
const ACTIONS_GROUP: Themed = Themed(0x420C0C0C, 0xFFE8E8E8);
const ACTIONS_SEPARATOR: Themed = Themed(0x06FFFFFF, 0xFFE2E2E2);
const ACTIONS_ROW: Themed = Themed(0xFFA8A8A8, 0xFF393939);
const ACTIONS_DISABLED: Themed = Themed(0xFF5E5E5E, 0xFFABABAB);
const SWITCH_FIXED: Themed = Themed(0xFF404040, 0xFFD2D2D2);
const SWITCH_FIXED_KNOB: Themed = Themed(0xFF525252, 0xFFE9E9E9);
const SLIDE_LABEL: Themed = Themed(0xFFC1C1C1, 0xFF3D3D3D);
const SLIDE_VALUE: Themed = Themed(0xFFEDEDED, 0xFF3D3D3D);
const SLIDE_TRACK: Themed = Themed(0xFF414141, 0xFFBEBEBE);
const CURSOR_HEADING: Themed = Themed(0xFFB5B5B5, 0xFF404040);
/// Brush scaling's menu: its fill and rim, a radio's ring.
const CHOICE_MENU: Glass =
    Glass { fill: Themed(0xFF5A5C5B, 0xFFF0F0F0), edge: Some(0x33FFFFFF), frost: false, corner: 26.0, shadow: false };
const RADIO_RING: Themed = Themed(0xFF606060, 0xFFB0B0B0);
const ROW_HEIGHT: f32 = 44.0;
const CHOICE_HEIGHT: f32 = 42.0;
const SLIDE_HEIGHT: f32 = 63.0;
const GROUP_GAP: f32 = 20.0;
const HEADING_HEIGHT: f32 = 45.0;
/// A card on the Plugins page: above its description, its problem's line, under it,
/// how far in, and its tag; and the page with nothing on it.
const PLUGIN_HEAD: f32 = 40.0;
const PLUGIN_PROBLEM: f32 = 22.0;
const PLUGIN_FOOT: f32 = 12.0;
const PLUGINS_EMPTY: f32 = 60.0;
const PLUGIN_INSET: f32 = 16.0;
/// The panel ends this far above the window's bottom edge, and its rows scroll past this tall.
const BOTTOM_MARGIN: f32 = 25.0;
const MOST_ROWS: f32 = 760.0;
/// A row with a private action: how far a swipe to the left slides it open, that action's button
/// and its look, and how long it takes to settle open or closed (seconds).
const PRIVATE_SLIDE: f32 = 248.0;
const PRIVATE_WIDTH: f32 = 245.0;
const PRIVATE_FILL: u32 = 0xFF454545;
const PRIVATE_INK: u32 = 0xFFA8A8A8;
const PRIVATE_SETTLE_S: f32 = 0.3;
/// A page pushed in or going back, and the rows fading out and in as the tab changes (seconds).
const PAGE_SLIDE_S: f32 = 0.45;
const TAB_OUT_S: f32 = 0.1;
const TAB_IN_S: f32 = 0.2;

/// What a row does. Those the panel does itself (a page, a menu, a setting) never reach the app;
/// [`Act::Prefs`] tells it a setting changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    TipAttachment,
    InsertFile,
    InsertPhoto,
    /// Insert a file or a photo, or Paste, swiped open: what they bring goes on a private layer.
    InsertPrivateFile,
    InsertPrivatePhoto,
    PastePrivate,
    /// On a model: its 2D texture shown flat, strokes reaching through it, the lighting studio.
    ModelFlat,
    PaintThrough,
    LightingStudio,
    AddText,
    Flip {
        horizontal: bool,
    },
    Share(Format),
    AnimatedExport(Animated),
    /// A model's turntable as an animation.
    ModelAnimated(Animated),
    About,
    Licences,
    AdvancedSettings,
    /// The keyboard shortcuts' list.
    Shortcuts,
    /// A plugin file to pick and ask about.
    ImportPlugin,
    /// A plugin's action tapped on its card: [`ActionsPanel::take_plugin_action`] says which.
    PluginAction,
    /// A file a plugin makes of the artwork's layers, by its place in [`Showing::plugin_exports`].
    PluginExport(u8),
    /// A row a plugin put in the Canvas tab, by its place among [`Showing::plugin_controls`] and
    /// then [`Showing::plugin_actions`]: a switch flipped, a setting named at every step moved to
    /// the next, an action run.
    PluginPlaced(u8),
    /// Edit Layout, while a plugin lets the bars move: their handles show.
    EditLayout,
    GestureControls,
    CanvasInfo,
    AnimationAssist,
    PageAssist,
    TimeLapseReplay,
    TimeLapseRecording,
    ExportTimeLapse,
    Reference,
    DrawingGuide,
    EditGuide,
    Crop,
    Cut,
    Copy,
    CopyCanvas,
    Paste,
    SideBar,
    Prefs,
    Page(Page),
    ScalingMenu,
    LightInterface,
    RightHanded,
    BrushCursor,
    ProjectCanvas,
    ResetInput,
}

/// The panel's pages: the tabs, or one Prefs opened.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Tabs,
    Pressure,
    Cursor,
    Plugins,
    /// What the pen's side buttons do.
    PenButtons,
}

/// The settings a slider row sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Slid {
    RapidUndo,
    SelectionVisibility,
    Stabilization,
    MotionFiltering,
    MotionExpression,
    /// A plugin's slider in the Canvas tab, by its place in [`Showing::plugin_controls`].
    Plugin(u8),
}

enum Row {
    /// A button row, dimmed without anything to do; its value on the right.
    Button(&'static str, Option<&'static str>, Option<Act>),
    /// A button row a swipe to the left slides open, a second button coming in from the right
    /// that does it privately: the two labels and what each does (none: dimmed).
    Private(&'static str, &'static str, Option<Act>, Option<Act>),
    /// A switch row, on or off; what a tap does, or none where the desktop can't change it yet
    /// (shown dimmed, as it is).
    Switch(&'static str, bool, Option<Act>),
    /// A slider row this tall: its name, the value as shown, where it is (0..1), and what it sets.
    Slide(&'static str, String, f32, f32, Slid),
    Heading(&'static str),
}

impl Row {
    fn height(&self) -> f32 {
        match self {
            Row::Heading(_) => HEADING_HEIGHT,
            Row::Slide(.., tall, _) => *tall,
            _ => ROW_HEIGHT,
        }
    }
}

/// The groups of a model's tab `tab` (Add, Canvas, Share or 3D).
fn model_groups(tab: usize, showing: &Showing) -> Vec<Vec<Row>> {
    use Row::{Button, Heading, Switch};
    // showing the 2D texture, the rest is for the model
    let shown = !showing.model_flat;
    match tab {
        0 => vec![
            vec![
                Button("Insert a file", None, Some(Act::InsertFile)),
                Button("Insert a photo", None, Some(Act::InsertPhoto)),
                Button("Take a photo", None, None),
            ],
            vec![
                Button("Cut", None, Some(Act::Cut)),
                Button("Copy", None, Some(Act::Copy)),
                Button("Copy canvas", None, Some(Act::CopyCanvas)),
                Button("Paste", None, showing.can_paste.then_some(Act::Paste)),
            ],
        ],
        1 => vec![
            vec![Switch("Reference", showing.reference, Some(Act::Reference))],
            vec![Button("Canvas information", None, Some(Act::CanvasInfo))],
        ],
        2 => vec![
            vec![
                Heading("Share Model"),
                // ours comes back whole in another copy of the app; the other package holds no
                // model yet
                Button("ProCraft", None, Some(Act::Share(Format::ModelArtwork))),
                Button("Procreate", None, None),
                Button("USDZ", None, Some(Act::Share(Format::ModelUsdz))),
                Button("OBJ", None, Some(Act::Share(Format::ModelObj))),
            ],
            vec![
                Heading("Share Image"),
                Button("JPEG", None, Some(Act::Share(Format::ModelJpeg))),
                Button("PNG", None, Some(Act::Share(Format::ModelRender))),
                Button("TIFF", None, Some(Act::Share(Format::ModelTiff))),
                Button("Animated GIF", None, Some(Act::ModelAnimated(Animated::Gif))),
                Button("Animated PNG", None, Some(Act::ModelAnimated(Animated::Png))),
                Button("Animated MP4", None, Some(Act::ModelAnimated(Animated::Mp4))),
                Button("Animated HEVC", None, Some(Act::ModelAnimated(Animated::Hevc))),
            ],
            vec![Heading("Share Textures"), Button("PNG", None, Some(Act::Share(Format::ModelTextures)))],
        ],
        _ => vec![
            vec![
                Switch("Show 2D texture", showing.model_flat, Some(Act::ModelFlat)),
                Switch("Paint through mesh", showing.paint_through, shown.then_some(Act::PaintThrough)),
                Button("Edit lighting & environment", None, shown.then_some(Act::LightingStudio)),
            ],
            vec![Button("View in AR", None, None)],
        ],
    }
}

/// What a tap in a group did.
#[derive(Clone, Copy)]
enum Tapped {
    Act(Act),
    Slid(Slid, f32),
}

const TABS: [(&str, &icons::Icon); 6] = [
    ("Add", &icons::TAB_ADD),
    ("Canvas", &icons::TAB_CANVAS),
    ("Share", &icons::TAB_SHARE),
    ("Video", &icons::TAB_VIDEO),
    ("Prefs", &icons::TAB_PREFS),
    ("Help", &icons::TAB_HELP),
];

/// On a model, its own tab takes Video's place.
const MODEL_TABS: [(&str, &icons::Icon); 6] = [
    ("Add", &icons::TAB_ADD),
    ("Canvas", &icons::TAB_CANVAS),
    ("Share", &icons::TAB_SHARE),
    ("3D", &icons::TAB3_D),
    ("Prefs", &icons::TAB_PREFS),
    ("Help", &icons::TAB_HELP),
];

/// What the rows show besides the settings: something to paste, and Animation or Page Assist,
/// Reference, the drawing guide and the side bar on.
pub struct Showing {
    pub can_paste: bool,
    pub animation: bool,
    pub pages: bool,
    pub time_lapse: bool,
    /// Something has been recorded (Replay and Export need it).
    pub time_lapse_recorded: bool,
    pub reference: bool,
    pub guide: bool,
    pub side_bar: bool,
    /// The artwork paints on a model: whether its 2D texture shows flat, and strokes reach
    /// through it.
    pub model: bool,
    pub model_flat: bool,
    pub paint_through: bool,
    /// The artwork is a plugin's: Share Image offers SVG too.
    pub svg: bool,
    /// What Share Image calls the files plugins make of the artwork's layers of kinds they add.
    pub plugin_exports: Vec<String>,
    /// The controls and actions plugins put in the Canvas tab, after its own rows.
    pub plugin_controls: Vec<PluginControl>,
    pub plugin_actions: Vec<PluginAction>,
    /// A plugin lets the bars move: Edit Layout follows those rows.
    pub edit_layout: bool,
}

/// The rows of what plugins put in the Canvas tab: each control as its card shows it, then each
/// action; none for none.
fn plugin_placed_rows(showing: &Showing) -> Vec<Row> {
    let controls = showing.plugin_controls.iter().map(|control| match control {
        PluginControl::Switch { control } => Row::Switch(kept_label(&control.label), control.on, None),
        PluginControl::Slider { control } if !plugin_slider_choices(control.clone()).is_empty() => {
            let shown = plugin_slider_shown(control.clone(), control.value);
            Row::Button(kept_label(&control.label), Some(kept_label(&shown)), None)
        }
        PluginControl::Slider { control } => Row::Slide(
            kept_label(&control.label),
            plugin_slider_shown(control.clone(), control.value),
            plugin_slider_along(control.clone(), control.value),
            SLIDE_HEIGHT,
            Slid::Plugin(0),
        ),
        PluginControl::File { control } => Row::Button(kept_label(&control.label), None, None),
        PluginControl::Texture { control } => Row::Button(kept_label(&control.label), None, None),
    });
    let actions = showing.plugin_actions.iter().map(|action| Row::Button(kept_label(&action.label), None, None));
    // each by its place, as far as a number counts
    controls
        .chain(actions)
        .take(usize::from(u8::MAX))
        .enumerate()
        .map(|(i, row)| {
            let at = Act::PluginPlaced(i as u8);
            match row {
                Row::Switch(label, on, _) => Row::Switch(label, on, Some(at)),
                Row::Slide(label, shown, along, tall, _) => {
                    Row::Slide(label, shown, along, tall, Slid::Plugin(i as u8))
                }
                Row::Button(label, value, _) => Row::Button(label, value, Some(at)),
                other => other,
            }
        })
        .collect()
}

/// `label` for as long as the app runs, each once: a row's words are kept so.
fn kept_label(label: &str) -> &'static str {
    static KEPT: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut kept = KEPT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(found) = kept.iter().find(|k| **k == label) {
        return found;
    }
    let leaked: &'static str = Box::leak(label.to_owned().into_boxed_str());
    kept.push(leaked);
    leaked
}

/// The groups of rows of tab `tab`.
fn groups(tab: usize, showing: &Showing, prefs: &Prefs) -> Vec<Vec<Row>> {
    use Row::{Button, Heading, Private, Slide, Switch};
    if showing.model && tab <= 3 {
        return model_groups(tab, showing);
    }
    match tab {
        0 => vec![
            vec![
                // swiped to the left, the first three and Paste put what they bring on a private
                // layer
                Private("Insert a file", "Insert a private file", Some(Act::InsertFile), Some(Act::InsertPrivateFile)),
                Private(
                    "Insert a photo",
                    "Insert a private photo",
                    Some(Act::InsertPhoto),
                    Some(Act::InsertPrivatePhoto),
                ),
                Private("Take a photo", "Take a private photo", None, None),
                Button("Add text", None, Some(Act::AddText)),
            ],
            vec![
                Button("Cut", None, Some(Act::Cut)),
                Button("Copy", None, Some(Act::Copy)),
                Button("Copy canvas", None, Some(Act::CopyCanvas)),
                Private(
                    "Paste",
                    "Paste Private",
                    showing.can_paste.then_some(Act::Paste),
                    showing.can_paste.then_some(Act::PastePrivate),
                ),
            ],
        ],
        1 => vec![
            vec![Button("Crop & Resize", None, Some(Act::Crop))],
            vec![
                Switch("Animation Assist", showing.animation, Some(Act::AnimationAssist)),
                Switch("Page Assist", showing.pages, Some(Act::PageAssist)),
                Switch("Drawing Guide", showing.guide, Some(Act::DrawingGuide)),
                // greyed until the guide is on
                Button("Edit Drawing Guide", None, showing.guide.then_some(Act::EditGuide)),
            ],
            vec![Switch("Reference", showing.reference, Some(Act::Reference))],
            vec![
                Button("Flip horizontal", None, Some(Act::Flip { horizontal: true })),
                Button("Flip vertical", None, Some(Act::Flip { horizontal: false })),
            ],
            vec![Button("Canvas information", None, Some(Act::CanvasInfo))],
        ]
        .into_iter()
        // what plugins put here
        .chain(Some(plugin_placed_rows(showing)).filter(|rows| !rows.is_empty()))
        // and Edit Layout, while a plugin lets the bars move
        .chain(showing.edit_layout.then(|| vec![Button("Edit Layout", None, Some(Act::EditLayout))]))
        .collect(),
        2 => vec![
            vec![
                Heading("Share Image"),
                Button("ProCraft", None, Some(Act::Share(Format::Procraft))),
                Button("Procreate", None, Some(Act::Share(Format::Package))),
                Button("PSD", None, Some(Act::Share(Format::Psd))),
                Button("PDF", None, Some(Act::Share(Format::Pdf))),
                Button("JPEG", None, Some(Act::Share(Format::Jpeg))),
                Button("PNG", None, Some(Act::Share(Format::Png))),
                Button("TIFF", None, Some(Act::Share(Format::Tiff))),
            ]
            .into_iter()
            .chain(showing.svg.then_some(Button("SVG", None, Some(Act::Share(Format::Svg)))))
            // files plugins make of the artwork's layers, under their own names
            .chain(
                showing
                    .plugin_exports
                    .iter()
                    .enumerate()
                    .take(usize::from(u8::MAX))
                    .map(|(i, label)| Button(kept_label(label), None, Some(Act::PluginExport(i as u8)))),
            )
            .collect(),
            vec![
                Heading("Share Layers"),
                Button("PDF", None, Some(Act::Share(Format::LayersPdf))),
                Button("PNG Files", None, Some(Act::Share(Format::LayerPngs))),
                Button("Animated GIF", None, Some(Act::AnimatedExport(Animated::Gif))),
                Button("Animated PNG", None, Some(Act::AnimatedExport(Animated::Png))),
                Button("Animated MP4", None, Some(Act::AnimatedExport(Animated::Mp4))),
                Button("Animated HEVC", None, Some(Act::AnimatedExport(Animated::Hevc))),
            ],
        ],
        3 => vec![
            vec![
                Button("Time-lapse Replay", None, showing.time_lapse_recorded.then_some(Act::TimeLapseReplay)),
                Switch("Time-lapse Recording", showing.time_lapse, Some(Act::TimeLapseRecording)),
            ],
            vec![Button("Export time-lapse video", None, showing.time_lapse_recorded.then_some(Act::ExportTimeLapse))],
        ],
        4 => {
            let delay = prefs.rapid_undo_delay;
            let visibility = prefs.selection_visibility;
            vec![
                vec![
                    Switch("Light interface", prefs.light_interface, Some(Act::LightInterface)),
                    Switch("Right-hand interface", prefs.right_handed, Some(Act::RightHanded)),
                    Button(
                        "Brush scaling",
                        Some(if prefs.brush_on_screen { "Screen" } else { "Canvas" }),
                        Some(Act::ScalingMenu),
                    ),
                    Switch("Project canvas", prefs.project_canvas, Some(Act::ProjectCanvas)),
                    Switch("Brush cursor", prefs.brush_cursor, Some(Act::BrushCursor)),
                    Button("Advanced cursor settings", None, Some(Act::Page(Page::Cursor))),
                ],
                vec![
                    Button("Pen buttons", None, Some(Act::Page(Page::PenButtons))),
                    Button("Pressure and Smoothing", None, Some(Act::Page(Page::Pressure))),
                    Button("Gesture controls", None, Some(Act::GestureControls)),
                    Button("Plugins", None, Some(Act::Page(Page::Plugins))),
                ],
                vec![
                    Slide(
                        "Rapid undo delay",
                        format!("{delay:.2}s"),
                        (delay - RAPID_UNDO_MIN_S) / (RAPID_UNDO_MAX_S - RAPID_UNDO_MIN_S),
                        SLIDE_HEIGHT,
                        Slid::RapidUndo,
                    ),
                    Slide(
                        "Selection mask visibility",
                        format!("{:.1}%", visibility * 100.0),
                        visibility,
                        SLIDE_HEIGHT,
                        Slid::SelectionVisibility,
                    ),
                ],
                vec![Switch("Size & Opacity sidebar", showing.side_bar, Some(Act::SideBar))],
            ]
        }
        _ => vec![
            vec![Button("About HanaCreate", None, Some(Act::About)), Button("Licences", None, Some(Act::Licences))],
            vec![
                Button("Keyboard shortcuts", None, Some(Act::Shortcuts)),
                Button("Advanced settings", None, Some(Act::AdvancedSettings)),
            ],
        ],
    }
}

/// Pressure and Smoothing's sliders.
fn smoothing_rows(prefs: &Prefs) -> Vec<Row> {
    let percent = |v: f32| if v <= 0.0 { "None".to_owned() } else { format!("{:.1}%", v * 100.0) };
    vec![
        Row::Slide("Stabilization", percent(prefs.stabilization), prefs.stabilization, 64.0, Slid::Stabilization),
        Row::Slide(
            "Motion Filtering",
            percent(prefs.motion_filtering),
            prefs.motion_filtering,
            64.0,
            Slid::MotionFiltering,
        ),
        Row::Slide(
            "Motion Filtering Expression",
            percent(prefs.motion_expression),
            prefs.motion_expression,
            64.0,
            Slid::MotionExpression,
        ),
    ]
}

/// A group's height: its heading, its rows and the lines between them (none between sliders).
fn group_height(rows: &[Row]) -> f32 {
    let mut height = 0.0;
    let mut last: Option<&Row> = None;
    for row in rows {
        if let Row::Heading(_) = row {
            height += HEADING_HEIGHT;
            continue;
        }
        if last.is_some_and(|l| !(matches!(l, Row::Slide(..)) && matches!(row, Row::Slide(..)))) {
            height += 1.0;
        }
        height += row.height();
        last = Some(row);
    }
    height
}

/// A choice group's height: its rows and the lines between those beside no chosen one.
fn choices_height(n: usize, chosen: usize) -> f32 {
    let lines = (1..n).filter(|&i| i != chosen && i - 1 != chosen).count();
    CHOICE_HEIGHT * n as f32 + lines as f32
}

#[derive(Default)]
pub struct Actions {
    /// The tab open (kept between runs in the settings).
    tab: Option<usize>,
    /// The tab whose rows show, and since when they change: the old rows fade out, then the new
    /// ones fade in.
    shown_tab: Option<usize>,
    tab_fade: Option<(Instant, usize)>,
    page: Page,
    /// The page last shown, and a page pushed in or going back: the page it leaves, since when,
    /// and whether it went forward.
    last_page: Page,
    page_move: Option<(Page, Instant, bool)>,
    /// Brush scaling's menu, and the row it hangs from.
    scaling: Menu,
    scaling_row: Option<Rect>,
    /// Pressure and Smoothing's curve.
    curve: crate::curve::CurveEditor,
    /// The artwork paints on a model: its tabs have 3D in Video's place.
    model_tabs: bool,
    /// The rows with a private action swiped (by group and row), until the tab changes or the
    /// panel closes.
    swipes: Swipes,
    /// A plugin's or plugin's slider under the pointer: its key, and where it has gone.
    plugin_moved: Option<(String, f32)>,
    /// The key of a plugin's action tapped, for the app to run.
    plugin_action: Option<String>,
    /// Pen buttons: the button whose choices are listed (true: the second), or none.
    pen_picking: Option<bool>,
    /// A plugin's choice menu, and the setting it picks for.
    plugin_menu: Menu,
    plugin_choice: Option<PluginChoice>,
    /// A plugin's texture row clicked: the control's key, for the Textures sheet (interface 15).
    textures_asked: Option<String>,
    /// Remove tapped on a plugin's card: the question it asks, and the plugin's id.
    removing: Alert,
    remove_what: Option<String>,
    /// A press on a plugin card's head, followed until let go.
    card_press: Option<CardPress>,
    /// Where each card was last drawn, by its place on the page, for a lifted card to land.
    card_rects: Vec<(usize, Rect)>,
}

/// A press on a plugin card's head, followed until let go: whose card (its place and id), where and
/// when it went down, where on the card it took hold, where the pointer is now, whether it was held
/// still long enough to lift the card, and whether it moved first (then the list has it, to scroll).
#[derive(Clone)]
struct CardPress {
    card: usize,
    id: String,
    origin: Pos2,
    since: Instant,
    grab: egui::Vec2,
    last: Pos2,
    lifted: bool,
    spent: bool,
}

/// A plugin's setting whose every step has a name, as a row that opens a menu of them: the
/// setting, the row it hangs from, and whether the row was just clicked.
#[derive(Clone)]
struct PluginChoice {
    slider: PluginSlider,
    row: Rect,
    clicked: bool,
}

type Swipes = HashMap<(usize, usize), Swipe>;

/// A private row's slide: under a drag, where it is; otherwise settling from `from` towards open
/// or closed since `since` (seconds).
#[derive(Clone, Copy, Debug, Default)]
struct Swipe {
    dragging: Option<f32>,
    from: f32,
    open: bool,
    since: f64,
}

impl Swipe {
    /// How far it is slid open at `now` (0 closed, [`PRIVATE_SLIDE`] open).
    fn at(&self, now: f64) -> f32 {
        if let Some(d) = self.dragging {
            return d;
        }
        let to = if self.open { PRIVATE_SLIDE } else { 0.0 };
        let k = ((now - self.since) as f32 / PRIVATE_SETTLE_S).clamp(0.0, 1.0);
        self.from + (to - self.from) * crate::look::spring(PRIVATE_SETTLE_S, 0.8, 0.2, k)
    }

    /// Settling open or closed from where it is.
    fn settle(&mut self, open: bool, now: f64) {
        self.from = self.at(now);
        self.dragging = None;
        self.open = open;
        self.since = now;
    }
}

impl Actions {
    /// Row `i` of what plugins put in the Canvas tab ([`Act::PluginPlaced`]) tapped: a switch flipped
    /// or a named setting moved to its next name (a setting changed), or an action to run.
    fn plugin_placed(&mut self, showing: &Showing, prefs: &mut Prefs, i: u8) -> Option<Act> {
        let i = usize::from(i);
        let Some(control) = showing.plugin_controls.get(i) else {
            let action = showing.plugin_actions.get(i.checked_sub(showing.plugin_controls.len())?)?;
            self.plugin_action = Some(action.key.clone());
            return Some(Act::PluginAction);
        };
        let changed = match control {
            PluginControl::Switch { control } => prefs.plugins.set_switch(control.key.clone(), !control.on),
            PluginControl::Slider { control } => {
                let count = plugin_slider_choices(control.clone()).len().max(1) as u32;
                let next = (plugin_slider_chosen(control.clone(), control.value) + 1) % count;
                let to = plugin_slider_choice(control.clone(), next);
                prefs.plugins.set_slider(control.key.clone(), to).is_some()
            }
            PluginControl::File { .. } | PluginControl::Texture { .. } => false,
        };
        changed.then_some(Act::Prefs)
    }

    /// The key of the plugin action last tapped, once ([`Act::PluginAction`]).
    pub fn take_plugin_action(&mut self) -> Option<String> {
        self.plugin_action.take()
    }

    /// The key of the texture control whose row was last clicked, once: its Textures sheet opens.
    pub fn take_textures_asked(&mut self) -> Option<String> {
        self.textures_asked.take()
    }

    /// Shows the panel hanging from `button`; returns where it is (points) and what a row asked. A setting changed in `prefs` returns
    /// [`Act::Prefs`].
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        button: Rect,
        showing: &Showing,
        prefs: &mut Prefs,
        backdrops: &mut Vec<f32>,
    ) -> (Rect, Option<Act>) {
        let screen = panel::screen(ctx);
        let (left, arrow_x) = panel::place(ctx, ACTIONS_WIDTH, button.center().x);
        let top = screen.top() + pc::BAR_HEIGHT;
        let tab = *self.tab.get_or_insert(prefs.actions_tab);
        let rows_alpha = self.tab_fade(ctx, tab);
        let groups = groups(self.shown_tab.unwrap_or(tab), showing, prefs);
        self.model_tabs = showing.model;
        let header = 12.0 + 44.0;
        let content_of = |page: Page| {
            panel::ARROW_HEIGHT
                + match page {
                    Page::Tabs => {
                        let rows: f32 = groups.iter().map(|g| group_height(g)).sum::<f32>()
                            + GROUP_GAP * (groups.len().saturating_sub(1)) as f32;
                        22.0 + 34.0 + 60.0 + 20.0 + rows.min(MOST_ROWS) + 11.0
                    }
                    Page::Cursor => {
                        let heading = 21.0 + 8.0;
                        header
                            + 8.0
                            + 6.0
                            + heading
                            + choices_height(3, prefs.cursor_shows as usize)
                            + 28.0
                            + heading
                            + choices_height(3, prefs.cursor_style as usize)
                            + 13.0
                    }
                    Page::Pressure => {
                        let curve = ACTIONS_WIDTH - 24.0 - 14.0;
                        let rows = 20.0
                            + group_height(&smoothing_rows(prefs))
                            + 16.0
                            + ROW_HEIGHT
                            + 24.0
                            + 19.0
                            + curve
                            + 27.0
                            + ROW_HEIGHT
                            + 13.0;
                        header + rows.min(MOST_ROWS)
                    }
                    Page::PenButtons => {
                        let rows = match self.pen_picking {
                            Some(_) => {
                                let plugins = prefs.plugins.actions_in(procraft_core::PluginPlace::Pen).len();
                                let n = procraft_core::pen_button_choices().len() + plugins;
                                8.0 + choices_height(n, 0) + 13.0
                            }
                            None => 8.0 + 2.0 * (6.0 + 21.0 + 8.0 + CHOICE_HEIGHT) + 13.0,
                        };
                        header + rows.min(MOST_ROWS)
                    }
                    Page::Plugins => {
                        let painter = ctx.layer_painter(egui::LayerId::background());
                        let list = prefs.plugins.plugins();
                        let cards: f32 = list.iter().map(|p| plugin_height(&painter, p, ACTIONS_WIDTH - 24.0)).sum();
                        let gaps = GROUP_GAP * list.len().saturating_sub(1) as f32;
                        let rows = if list.is_empty() { PLUGINS_EMPTY } else { cards + gaps };
                        header + 8.0 + (rows + 13.0).min(MOST_ROWS)
                    }
                }
        };
        // a page pushed in slides in from the right as the one it covers slides off to the left;
        // going back, the page goes at once as the one under it slides back in; the panel's
        // height follows
        let moving = self.page_move.map(|(from, since, forward)| {
            let t = since.elapsed().as_secs_f32() / PAGE_SLIDE_S;
            (from, cubic_bezier(0.25, 0.1, 0.25, 1.0, t), forward, t >= 1.0)
        });
        let content = match moving {
            Some((from, e, _, _)) => content_of(from) + (content_of(self.page) - content_of(from)) * e,
            None => content_of(self.page),
        };
        if moving.is_some_and(|m| m.3) {
            self.page_move = None;
        } else if moving.is_some() {
            ctx.request_repaint();
        }
        let height = content.min(screen.bottom() - BOTTOM_MARGIN - top);
        let rect = Rect::from_min_size(pos2(left, top), vec2(ACTIONS_WIDTH, height));
        let rect = panel::hung_rect(ctx, rect, content, BOTTOM_MARGIN);
        let mut tapped = None;
        let page = self.page;
        panel::area(ctx, Id::unique("actions panel"), rect).show(ctx, |ui| {
            ui.allocate_rect(rect, Sense::click());
            panel::frame(ui, look, "actions frame", rect, arrow_x, backdrops);
            let body = panel::body_in(ui.ctx(), rect);
            ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
                ui.set_clip_rect(body.intersect(ui.clip_rect()));
                let width = body.width();
                let (shift, leaving) = match moving {
                    Some((from, e, true, false)) => (width * (1.0 - e), Some((from, -width * e))),
                    Some((_, e, false, false)) => (-width * (1.0 - e), None),
                    _ => (0.0, None),
                };
                // the page it leaves, only seen
                if let Some((from, dx)) = leaving {
                    let _ = self.page_view(ui, look, from, body.translate(vec2(dx, 0.0)), &groups, prefs, rows_alpha);
                }
                tapped = self.page_view(ui, look, page, body.translate(vec2(shift, 0.0)), &groups, prefs, rows_alpha);
                if leaving.is_some() || shift != 0.0 {
                    tapped = None;
                }
            });
        });
        // what the panel does itself
        let mut act = match tapped {
            Some(Tapped::Act(Act::Page(page))) => {
                self.page = page;
                self.pen_picking = None;
                None
            }
            Some(Tapped::Act(Act::ScalingMenu)) => {
                self.scaling.open();
                None
            }
            Some(Tapped::Act(Act::LightInterface)) => {
                prefs.light_interface = !prefs.light_interface;
                Some(Act::Prefs)
            }
            Some(Tapped::Act(Act::TipAttachment)) => {
                prefs.tip_attached = !prefs.tip_attached;
                Some(Act::Prefs)
            }
            Some(Tapped::Act(Act::RightHanded)) => {
                prefs.right_handed = !prefs.right_handed;
                Some(Act::Prefs)
            }
            Some(Tapped::Act(Act::BrushCursor)) => {
                prefs.brush_cursor = !prefs.brush_cursor;
                Some(Act::Prefs)
            }
            Some(Tapped::Act(Act::ProjectCanvas)) => {
                prefs.project_canvas = !prefs.project_canvas;
                Some(Act::Prefs)
            }
            Some(Tapped::Act(Act::ResetInput)) => {
                prefs.reset_input();
                Some(Act::Prefs)
            }
            Some(Tapped::Act(Act::PluginPlaced(i))) => self.plugin_placed(showing, prefs, i),
            Some(Tapped::Act(a)) => Some(a),
            Some(Tapped::Slid(Slid::Plugin(i), at)) => {
                let slider = match showing.plugin_controls.get(usize::from(i)) {
                    Some(PluginControl::Slider { control }) => Some(control.clone()),
                    _ => None,
                };
                let to = slider.map(|s| (s.key.clone(), plugin_slider_at(s, at.clamp(0.0, 1.0))));
                to.and_then(|(key, to)| prefs.plugins.set_slider(key, to)).map(|_| Act::Prefs)
            }
            Some(Tapped::Slid(which, at)) => {
                let at = at.clamp(0.0, 1.0);
                match which {
                    Slid::RapidUndo => {
                        prefs.rapid_undo_delay = RAPID_UNDO_MIN_S + at * (RAPID_UNDO_MAX_S - RAPID_UNDO_MIN_S);
                    }
                    Slid::SelectionVisibility => prefs.selection_visibility = at,
                    Slid::Stabilization => prefs.stabilization = at,
                    Slid::MotionFiltering => prefs.motion_filtering = at,
                    Slid::MotionExpression => prefs.motion_expression = at,
                    Slid::Plugin(_) => {}
                }
                Some(Act::Prefs)
            }
            None => None,
        };
        // another tab, kept
        if let Some(tab) = self.tab.filter(|t| *t != prefs.actions_tab) {
            prefs.actions_tab = tab;
            act = act.or(Some(Act::Prefs));
        }
        if self.scaling_menu(ctx, look, prefs, backdrops) {
            act = Some(Act::Prefs);
        }
        if self.page != Page::Plugins {
            self.plugin_menu.close();
        }
        if self.plugin_menu(ctx, look, prefs, backdrops) {
            act = Some(Act::Prefs);
        }
        if self.remove_question(ctx, look, prefs) {
            act = Some(Act::Prefs);
        }
        if self.page != Page::Pressure {
            self.curve.close();
        }
        if self.curve.menu(ctx, &mut prefs.curve) {
            act = Some(Act::Prefs);
        }
        // another page: it slides in
        if self.page != self.last_page {
            self.page_move = Some((self.last_page, Instant::now(), self.page != Page::Tabs));
            self.last_page = self.page;
            ctx.request_repaint();
        }
        (rect, act)
    }

    /// Page `page` of the panel in `body`, the tabs' rows shown `rows_alpha`.
    #[allow(clippy::too_many_arguments)]
    fn page_view(
        &mut self,
        ui: &mut Ui,
        look: &mut Look,
        page: Page,
        body: Rect,
        groups: &[Vec<Row>],
        prefs: &mut Prefs,
        rows_alpha: f32,
    ) -> Option<Tapped> {
        match page {
            Page::Tabs => self.tabs(ui, look, body, groups, rows_alpha),
            Page::Cursor => self.cursor_page(ui, look, body, prefs),
            Page::Pressure => self.pressure_page(ui, look, body, prefs),
            Page::Plugins => self.plugins_page(ui, look, body, prefs),
            Page::PenButtons => self.pen_buttons_page(ui, look, body, prefs),
        }
    }

    /// How much the rows show as the tab changes to `tab`: the old ones fade out (0.1 s), then
    /// the new ones fade in (0.2 s).
    fn tab_fade(&mut self, ctx: &egui::Context, tab: usize) -> f32 {
        let shown = *self.shown_tab.get_or_insert(tab);
        // another tab (also while the rows fade): they fade out from here
        if self.tab_fade.map_or(shown != tab, |(_, to)| to != tab) {
            self.tab_fade = Some((Instant::now(), tab));
        }
        let Some((since, _)) = self.tab_fade else { return 1.0 };
        ctx.request_repaint();
        let t = since.elapsed().as_secs_f32();
        let ease = |k: f32| cubic_bezier(0.42, 0.0, 0.58, 1.0, k);
        if t < TAB_OUT_S {
            return 1.0 - ease(t / TAB_OUT_S);
        }
        if self.shown_tab != Some(tab) {
            self.swipes.clear();
        }
        self.shown_tab = Some(tab);
        let k = (t - TAB_OUT_S) / TAB_IN_S;
        if k >= 1.0 {
            self.tab_fade = None;
            return 1.0;
        }
        ease(k)
    }

    /// One of its menus is open: a press elsewhere only closes it.
    pub fn modal(&self) -> bool {
        self.scaling.is_open() || self.plugin_menu.is_open() || self.curve.menu_open() || self.removing.is_open()
    }

    /// The panel's pages go back to the tabs (as it closes).
    pub fn closed(&mut self) {
        self.page = Page::Tabs;
        self.last_page = Page::Tabs;
        self.page_move = None;
        self.tab_fade = None;
        self.shown_tab = self.tab;
        self.scaling.close();
        self.plugin_menu.close();
        self.curve.close();
        self.swipes.clear();
        self.card_press = None;
    }

    /// The tabs page: the title, the tabs, and the open tab's groups.
    fn tabs(&mut self, ui: &mut Ui, look: &mut Look, body: Rect, groups: &[Vec<Row>], alpha: f32) -> Option<Tapped> {
        text(
            ui.painter(),
            pos2(body.left() + 24.0, body.top() + 22.0),
            Align2::LEFT_TOP,
            "Actions",
            24.0,
            Weight::Bold,
            argb(PANEL_TITLE),
        );
        // the tabs: 54 wide, 10 apart; a tab lights up as soon as it is touched
        let tabs_top = body.top() + 22.0 + 34.0;
        let tabs = if self.model_tabs { &MODEL_TABS } else { &TABS };
        for (i, (label, icon)) in tabs.iter().enumerate() {
            let cell = Rect::from_min_size(pos2(body.left() + 10.0 + 64.0 * i as f32, tabs_top), vec2(54.0, 60.0));
            let response = ui.interact(cell, ui.scope_id().with(("tab", i)), Sense::click());
            if response.clicked() {
                self.tab = Some(i);
            }
            let lit = Some(i) == self.tab || response.is_pointer_button_down_on();
            // a 50 frame centred 24 down
            let tint = if lit { argb(pc::ACCENT) } else { argb(TAB_IDLE) };
            look.icon(ui, icon, pos2(cell.center().x, cell.top() + 24.0), 50.0, tint);
            let ink = if lit { argb(pc::TEXT) } else { argb(TAB_IDLE) };
            let at = pos2(cell.center().x, cell.top() + 41.0);
            text(ui.painter(), at, Align2::CENTER_TOP, label, 12.0, Weight::Medium, ink);
        }
        let list = Rect::from_min_max(
            pos2(body.left() + 12.0, tabs_top + 60.0 + 20.0),
            pos2(body.right() - 12.0, body.bottom() - 11.0),
        );
        let mut tapped = None;
        self.scaling_row = None;
        let shown_tab = self.shown_tab;
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            ui.multiply_opacity(alpha);
            ScrollArea::vertical()
                .id_salt(("actions rows", shown_tab))
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for (g, group) in groups.iter().enumerate() {
                        if g > 0 {
                            ui.add_space(GROUP_GAP);
                        }
                        let (t, cells) = group_view(ui, group, g, &mut self.swipes);
                        tapped = tapped.or(t);
                        // Brush scaling's menu hangs beside its row
                        if let Some(at) = group
                            .iter()
                            .filter(|r| !matches!(r, Row::Heading(_)))
                            .position(|r| matches!(r, Row::Button(_, _, Some(Act::ScalingMenu))))
                        {
                            self.scaling_row = cells.get(at).copied();
                        }
                    }
                });
        });
        tapped
    }

    /// Brush scaling's menu, while open: relative to the canvas or to the screen, the chosen one
    /// marked; 254 wide, its left edge 54 in from the row's right end. True when one was picked.
    fn scaling_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        prefs: &mut Prefs,
        backdrops: &mut Vec<f32>,
    ) -> bool {
        let Some(row) = self.scaling_row else {
            self.scaling.close();
            return false;
        };
        let labels = ["Relative to canvas", "Relative to screen"];
        let chosen = usize::from(prefs.brush_on_screen);
        let Some(picked) = choice_menu(&mut self.scaling, ctx, look, backdrops, "brush scaling", row, &labels, chosen)
        else {
            return false;
        };
        prefs.brush_on_screen = picked == 1;
        self.scaling.close();
        true
    }

    /// A plugin's choice menu, while open: its setting's names, the chosen one marked. True when
    /// one was picked.
    fn plugin_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        prefs: &mut Prefs,
        backdrops: &mut Vec<f32>,
    ) -> bool {
        let Some(choice) = self.plugin_choice.as_mut() else {
            self.plugin_menu.close();
            return false;
        };
        if std::mem::take(&mut choice.clicked) {
            self.plugin_menu.open();
        }
        let PluginChoice { slider, row, .. } = choice.clone();
        let names = plugin_slider_choices(slider.clone());
        let labels: Vec<&str> = names.iter().map(String::as_str).collect();
        let chosen = plugin_slider_chosen(slider.clone(), slider.value) as usize;
        let Some(picked) =
            choice_menu(&mut self.plugin_menu, ctx, look, backdrops, "plugin choice", row, &labels, chosen)
        else {
            return false;
        };
        self.plugin_menu.close();
        prefs.plugins.set_slider(slider.key.clone(), plugin_slider_choice(slider, picked as u32)).is_some()
    }

    /// The question Remove on a plugin's card asks, while it is up: answered Remove, the plugin
    /// goes. True when it went.
    fn remove_question(&mut self, ctx: &egui::Context, look: &mut Look, prefs: &mut Prefs) -> bool {
        match self.removing.show(ctx, look) {
            Some(Answer::Action) => {
                let Some(id) = self.remove_what.take() else { return false };
                if prefs.plugins.remove(id).is_some() {
                    return true;
                }
                crate::files::say("This couldn't be removed.");
                false
            }
            Some(_) => {
                self.remove_what = None;
                false
            }
            None => false,
        }
    }

    /// Asks before `plugin` goes, its name in the question.
    fn ask_remove(&mut self, plugin: &PluginInfo) {
        self.remove_what = Some(plugin.id.clone());
        let message = format!("\u{201c}{}\u{201d} goes, and its settings with it.", plugin.name);
        self.removing.open("Remove this plugin?", &message, "Remove", Some(&icons::TRASH));
    }

    /// Follows the press on a plugin card's head: moved first, it is left to the list; held still,
    /// the card lifts and follows the pointer and, let go over another card, takes its place.
    fn follow_card_press(&mut self, ctx: &egui::Context, prefs: &Prefs) {
        let Some(mut p) = self.card_press.take() else { return };
        let (at, down) = ctx.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
        let at = at.map_or(p.last, |at| panel::in_panel(ctx, at));
        if down {
            if p.lifted {
                ctx.request_repaint();
            } else if !p.spent {
                if (at - p.origin).length() > crate::library::SLOP {
                    p.spent = true;
                } else if p.since.elapsed() >= crate::library::HOLD {
                    p.lifted = true;
                    ctx.request_repaint();
                } else {
                    ctx.request_repaint_after(crate::library::HOLD.saturating_sub(p.since.elapsed()));
                }
            }
            p.last = at;
            self.card_press = Some(p);
            return;
        }
        if !p.lifted {
            return;
        }
        if let Some(&(to, _)) = self.card_rects.iter().find(|(_, r)| r.contains(at))
            && to != p.card
            && let Some(words) = prefs.plugins.move_plugin(p.id, to as u32)
        {
            crate::files::say(&words);
        }
        ctx.request_repaint();
    }

    /// A page's header: the back chevron and its title. True when Back was tapped.
    fn page_header(&mut self, ui: &Ui, look: &mut Look, body: Rect, title: &str) -> bool {
        let back = Rect::from_min_size(pos2(body.left() + 8.0, body.top() + 12.0 + 5.0), vec2(44.0, 34.0));
        look.icon(ui, &icons::CHEVRON_LEFT, back.center(), 22.0, argb(PANEL_TITLE));
        let at = pos2(back.right(), body.top() + 12.0 + 22.0);
        text(ui.painter(), at, Align2::LEFT_CENTER, title, 22.0, Weight::Bold, argb(PANEL_TITLE));
        let tapped = ui.interact(back, ui.scope_id().with("back"), Sense::click()).clicked();
        if tapped {
            self.page = Page::Tabs;
            self.curve.close();
        }
        tapped
    }

    /// The plugins, a card each, drawn from what the list says of them. A slider's value goes in as
    /// it lets go, unless it is live: what it sets may move the slider.
    fn plugins_page(&mut self, ui: &mut Ui, look: &mut Look, body: Rect, prefs: &mut Prefs) -> Option<Tapped> {
        if self.page_header(ui, look, body, "Plugins") {
            return None;
        }
        let button = Rect::from_min_size(pos2(body.right() - 12.0 - 72.0, body.top() + 12.0 + 5.0), vec2(72.0, 34.0));
        let at = pos2(button.right() - 4.0, button.center().y);
        text(ui.painter(), at, Align2::RIGHT_CENTER, "Import", 17.0, Weight::Regular, argb(pc::ACCENT));
        let import = ui.interact(button, ui.scope_id().with("import plugin"), Sense::click()).clicked();
        let list = Rect::from_min_max(
            pos2(body.left() + 12.0, body.top() + 12.0 + 44.0 + 8.0),
            pos2(body.right() - 12.0, body.bottom()),
        );
        let shown = prefs.plugins.plugins();
        let mut changed = false;
        let mut remove = None;
        // a card held still lifts, and is dragged rather than the list
        let lifted = self.card_press.clone().filter(|p| p.lifted);
        let mut card_rects = Vec::new();
        let mut head_heights = Vec::new();
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            ScrollArea::vertical()
                .id_salt("plugin cards")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .scroll_source(if lifted.is_some() {
                    egui::scroll_area::ScrollSource::NONE
                } else {
                    egui::scroll_area::ScrollSource::default()
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    if shown.is_empty() {
                        let width = ui.available_width();
                        let (area, _) = ui.allocate_exact_size(vec2(width, PLUGINS_EMPTY), Sense::hover());
                        let words = "Nothing imported yet. Import a .procraft-plugin file to add one.";
                        let inside = width - 2.0 * PLUGIN_INSET;
                        let said = wrapped(ui.painter(), words, 15.0, Weight::Regular, argb(ACTIONS_ROW), inside);
                        ui.painter().galley(area.min + vec2(PLUGIN_INSET, 8.0), said, argb(ACTIONS_ROW));
                    }
                    for (g, plugin) in shown.iter().enumerate() {
                        if g > 0 {
                            ui.add_space(GROUP_GAP);
                        }
                        // a lifted card is faint where it was
                        let faint = lifted.as_ref().is_some_and(|p| p.card == g);
                        let (card_changed, head, area) = ui
                            .scope(|ui| {
                                if faint {
                                    ui.multiply_opacity(0.3);
                                }
                                plugin_card(
                                    ui,
                                    plugin,
                                    g,
                                    &prefs.plugins,
                                    &mut self.plugin_moved,
                                    &mut self.plugin_action,
                                    &mut self.plugin_choice,
                                    &mut self.textures_asked,
                                    &mut remove,
                                )
                            })
                            .inner;
                        changed |= card_changed;
                        card_rects.push((g, area));
                        head_heights.push(head.rect.height());
                        if self.card_press.is_none()
                            && head.is_pointer_button_down_on()
                            && let Some(p) = head.interact_pointer_pos()
                        {
                            self.card_press = Some(CardPress {
                                card: g,
                                id: plugin.id.clone(),
                                origin: p,
                                since: Instant::now(),
                                grab: p - area.min,
                                last: p,
                                lifted: false,
                                spent: false,
                            });
                        }
                    }
                    ui.add_space(13.0);
                });
            // the lifted card follows the pointer, over the list: its name and what it does
            if let Some(p) = &lifted
                && let (Some(plugin), Some(&(_, area)), Some(&height)) =
                    (shown.get(p.card), card_rects.get(p.card), head_heights.get(p.card))
            {
                let at = Rect::from_min_size(p.last - p.grab, vec2(area.width(), height));
                let painter = ui.painter_at(list);
                painter.rect_filled(at, 22, argb(pc::ACCENT).gamma_multiply(0.9));
                let left = at.left() + PLUGIN_INSET;
                let name_at = pos2(left, at.top() + 12.0);
                text(&painter, name_at, Align2::LEFT_TOP, &plugin.name, 17.0, Weight::SemiBold, Color32::WHITE);
                let inside = at.width() - 2.0 * PLUGIN_INSET;
                let said = wrapped(&painter, &plugin.description, 14.0, Weight::Regular, Color32::WHITE, inside);
                painter.galley(pos2(left, at.top() + PLUGIN_HEAD), said, Color32::WHITE);
            }
        });
        self.card_rects = card_rects;
        self.follow_card_press(ui.ctx(), prefs);
        if let Some(plugin) = remove {
            self.ask_remove(&plugin);
        }
        if !ui.input(|i| i.pointer.any_down())
            && let Some((key, value)) = self.plugin_moved.take()
        {
            changed |= prefs.plugins.set_slider(key, value).is_some();
        }
        if import {
            return Some(Tapped::Act(Act::ImportPlugin));
        }
        if self.plugin_action.is_some() {
            return Some(Tapped::Act(Act::PluginAction));
        }
        changed.then_some(Tapped::Act(Act::Prefs))
    }

    /// Pen buttons: what the pen's first and second side buttons do, a QuickMenu action each or, as
    /// they start, what Gesture controls say. A button's row tapped lists the choices, the one set
    /// blue; a choice tapped sets it and comes back.
    fn pen_buttons_page(&mut self, ui: &mut Ui, look: &mut Look, body: Rect, prefs: &mut Prefs) -> Option<Tapped> {
        let label = |a: Option<procraft_core::QuickAction>| {
            a.map_or("Gesture controls".to_owned(), procraft_core::quick_action_label)
        };
        let title = match self.pen_picking {
            None => "Pen buttons",
            Some(false) => "Pen button",
            Some(true) => "Pen second button",
        };
        // the plugins' actions a button can run, of those on: after the app's own, by their labels
        let plugin_actions = prefs.plugins.actions_in(procraft_core::PluginPlace::Pen);
        let plugin_set = |prefs: &Prefs, k: usize| {
            let key = prefs.pen_button_plugins.get(k).cloned().flatten()?;
            plugin_actions.iter().position(|a| a.key == key)
        };
        if self.page_header(ui, look, body, title) {
            // back from the choices to the buttons
            if self.pen_picking.take().is_some() {
                self.page = Page::PenButtons;
            }
            return None;
        }
        let (left, right) = (body.left() + 12.0, body.right() - 12.0);
        let top = body.top() + 12.0 + 44.0 + 8.0;
        let Some(second) = self.pen_picking else {
            let mut y = top;
            for (k, heading) in ["Pen button", "Pen second button"].into_iter().enumerate() {
                y += 6.0;
                text(
                    ui.painter(),
                    pos2(left + 14.0, y),
                    Align2::LEFT_TOP,
                    heading,
                    17.0,
                    Weight::Bold,
                    argb(CURSOR_HEADING),
                );
                y += 21.0 + 8.0;
                let row = Rect::from_min_size(pos2(left, y), vec2(right - left, CHOICE_HEIGHT));
                let set = match plugin_set(prefs, k).and_then(|at| plugin_actions.get(at)) {
                    Some(action) => action.label.clone(),
                    None => label(prefs.pen_buttons.get(k).copied().flatten()),
                };
                // one row saying what it is set to, none of it blue
                if choice_group(ui, row, &[set.as_str()], 1, 10 + k).is_some() {
                    self.pen_picking = Some(k == 1);
                }
                y = row.bottom();
            }
            return None;
        };
        let choices = procraft_core::pen_button_choices();
        let mut labels: Vec<String> = choices.iter().map(|&a| label(a)).collect();
        labels.extend(plugin_actions.iter().map(|a| a.label.clone()));
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let at = usize::from(second);
        let set = prefs.pen_buttons.get(at).copied().flatten();
        let chosen = match plugin_set(prefs, at) {
            Some(k) => choices.len() + k,
            None => choices.iter().position(|&c| c == set).unwrap_or(0),
        };
        let list = Rect::from_min_max(pos2(left, top), pos2(right, body.bottom()));
        let mut picked = None;
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            ScrollArea::vertical()
                .id_salt("pen button choices")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    let (group, _) = ui
                        .allocate_exact_size(vec2(right - left, choices_height(labels.len(), chosen)), Sense::hover());
                    picked = choice_group(ui, group, &labels, chosen, 20);
                    ui.add_space(13.0);
                });
        });
        let i = picked?;
        if prefs.pen_buttons.len() < 2 {
            prefs.pen_buttons.resize(2, None);
        }
        if prefs.pen_button_plugins.len() < 2 {
            prefs.pen_button_plugins.resize(2, None);
        }
        // a plugin's action in place of the app's, or the app's and none of a plugin's
        let plugin = i.checked_sub(choices.len()).and_then(|k| plugin_actions.get(k)).map(|a| a.key.clone());
        if let Some(slot) = prefs.pen_buttons.get_mut(at) {
            *slot = choices.get(i).copied().flatten();
        }
        if let Some(slot) = prefs.pen_button_plugins.get_mut(at) {
            *slot = plugin;
        }
        self.pen_picking = None;
        Some(Tapped::Act(Act::Prefs))
    }

    /// Advanced cursor settings: when the brush cursor shows, and how it is drawn.
    fn cursor_page(&mut self, ui: &mut Ui, look: &mut Look, body: Rect, prefs: &mut Prefs) -> Option<Tapped> {
        if self.page_header(ui, look, body, "Advanced cursor") {
            return None;
        }
        let (left, right) = (body.left() + 12.0, body.right() - 12.0);
        let mut y = body.top() + 12.0 + 44.0 + 8.0;
        let mut changed = false;
        for (k, heading) in ["Brush cursor visibility", "Brush outline style"].into_iter().enumerate() {
            y += if k == 0 { 6.0 } else { 28.0 };
            let ink = argb(CURSOR_HEADING);
            text(ui.painter(), pos2(left + 14.0, y), Align2::LEFT_TOP, heading, 17.0, Weight::Bold, ink);
            y += 21.0 + 8.0;
            let (labels, chosen) = if k == 0 {
                (["Show while hovering", "Show while painting", "Show both"], prefs.cursor_shows as usize)
            } else {
                (["High contrast", "Active color", "Per brush"], prefs.cursor_style as usize)
            };
            let group = Rect::from_min_size(pos2(left, y), vec2(right - left, choices_height(3, chosen)));
            if let Some(i) = choice_group(ui, group, &labels, chosen, k) {
                if k == 0 {
                    prefs.cursor_shows = match i {
                        0 => CursorShows::Hovering,
                        1 => CursorShows::Painting,
                        _ => CursorShows::Both,
                    };
                } else {
                    prefs.cursor_style = match i {
                        0 => CursorStyle::Contrast,
                        1 => CursorStyle::Colour,
                        _ => CursorStyle::Brush,
                    };
                }
                changed = true;
            }
            y = group.bottom();
        }
        changed.then_some(Tapped::Act(Act::Prefs))
    }

    /// Pressure and Smoothing: the app's stabilization, motion filtering and its expression, Tip
    /// Attachment, the pressure curve every stroke goes through, and Reset all.
    fn pressure_page(&mut self, ui: &mut Ui, look: &mut Look, body: Rect, prefs: &mut Prefs) -> Option<Tapped> {
        if self.page_header(ui, look, body, "Pressure & Smoothing") {
            return None;
        }
        let list = Rect::from_min_max(
            pos2(body.left() + 12.0, body.top() + 12.0 + 44.0),
            pos2(body.right() - 12.0, body.bottom()),
        );
        let mut tapped = None;
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            ScrollArea::vertical()
                .id_salt("pressure rows")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    let width = ui.available_width();
                    ui.add_space(20.0);
                    tapped = group_view(ui, &smoothing_rows(prefs), 0, &mut self.swipes).0;
                    ui.add_space(16.0);
                    // with no filtering it has nothing to do
                    let can = (prefs.motion_filtering > 0.0).then_some(Act::TipAttachment);
                    let tip = [Row::Switch("Tip Attachment", prefs.tip_attached, can)];
                    tapped = tapped.or(group_view(ui, &tip, 1, &mut self.swipes).0);
                    ui.add_space(24.0);
                    let (label, _) = ui.allocate_exact_size(vec2(width, 19.0), Sense::hover());
                    text(
                        ui.painter(),
                        pos2(label.left() + 13.0, label.center().y),
                        Align2::LEFT_CENTER,
                        "App Pressure Sensitivity",
                        13.0,
                        Weight::SemiBold,
                        argb(SLIDE_LABEL),
                    );
                    let (band, _) = ui.allocate_exact_size(vec2(width, width - 14.0), Sense::hover());
                    let square = band.shrink2(vec2(7.0, 0.0));
                    if self.curve.show(ui, square, &mut prefs.curve, &crate::curve::ACTIONS, "pressure") {
                        tapped = Some(Tapped::Act(Act::Prefs));
                    }
                    ui.add_space(27.0);
                    let reset = [Row::Button("Reset all", None, Some(Act::ResetInput))];
                    let (t, _) = group_view(ui, &reset, 2, &mut self.swipes);
                    tapped = tapped.or(t);
                    ui.add_space(13.0);
                });
        });
        tapped
    }
}

/// Whether a card's controls show: once the plugin is on.
fn controls_show(plugin: &PluginInfo) -> bool {
    plugin.on && plugin.problem.is_none()
}

/// A feature's head on a card (interface 16): its padding above and below, its name's height, and
/// the room its words leave for its switch.
const FEATURE_PAD: f32 = 10.0;
const FEATURE_NAME: f32 = 21.0;
const FEATURE_ROOM: f32 = 16.0 + 8.0 + 63.0 + 12.0;

/// How tall `feature`'s head is on a card `width` wide.
fn feature_head_height(painter: &Painter, feature: &procraft_core::PluginFeature, width: f32) -> f32 {
    let said = if feature.description.is_empty() {
        0.0
    } else {
        4.0 + wrapped(painter, &feature.description, 14.0, Weight::Regular, Color32::WHITE, width - FEATURE_ROOM)
            .size()
            .y
    };
    2.0 * FEATURE_PAD + FEATURE_NAME + said
}

/// How tall `control` is as a card's row, its line above it with it.
fn control_height(control: &PluginControl) -> f32 {
    match control {
        PluginControl::Switch { .. } | PluginControl::File { .. } | PluginControl::Texture { .. } => 1.0 + ROW_HEIGHT,
        PluginControl::Slider { control } if !plugin_slider_choices(control.clone()).is_empty() => 1.0 + ROW_HEIGHT,
        PluginControl::Slider { .. } => 1.0 + SLIDE_HEIGHT,
    }
}

/// A row under a card's controls.
#[derive(Clone, Copy)]
enum CardRow {
    /// Its action at this place in its list.
    Action(usize),
    TurnOn,
    TurnOff,
    Remove,
}

/// What can be done with `plugin`: what it does when tapped, once it's on; it is on or off; and
/// what was imported can go.
fn card_rows(plugin: &PluginInfo) -> Vec<CardRow> {
    // (a plugin of features has its actions under them)
    let flat = controls_show(plugin) && plugin.features.is_empty();
    let actions = (0..plugin.actions.len()).filter(|_| flat).map(CardRow::Action);
    let mut rows: Vec<CardRow> = actions.collect();
    rows.push(if plugin.on { CardRow::TurnOff } else { CardRow::TurnOn });
    if plugin.source == PluginSource::File {
        rows.push(CardRow::Remove);
    }
    rows
}

/// How tall `plugin`'s card is, `width` wide.
fn plugin_height(painter: &Painter, plugin: &PluginInfo, width: f32) -> f32 {
    let inside = width - 2.0 * PLUGIN_INSET;
    let said = wrapped(painter, &plugin.description, 14.0, Weight::Regular, Color32::WHITE, inside);
    let problem = if plugin.problem.is_some() { PLUGIN_PROBLEM } else { 0.0 };
    let controls: f32 = if !controls_show(plugin) {
        0.0
    } else if plugin.features.is_empty() {
        plugin.controls.iter().map(control_height).sum()
    } else {
        let under = |f: &procraft_core::PluginFeature| {
            let rows: f32 = f.controls.iter().map(control_height).sum();
            if f.control.on { rows + f.actions.len() as f32 * (1.0 + ROW_HEIGHT) } else { 0.0 }
        };
        plugin.features.iter().map(|f| 1.0 + feature_head_height(painter, f, width) + under(f)).sum()
    };
    let rows = card_rows(plugin).len() as f32 * (1.0 + ROW_HEIGHT);
    PLUGIN_HEAD + said.size().y + problem + PLUGIN_FOOT + controls + rows
}

/// `plugin` as a card: its name and tag, what it does, what is the matter with it, its controls
/// whatever they are, its actions, and the rows that apply or remove it. A switch flips at once, a
/// live slider goes in as it moves, another leaves where it has gone in `moved`; a slider named at
/// every step is a row whose click leaves it in `choice` for its menu (as does its row while that
/// is open); an action tapped leaves its key in `action`, and Remove the plugin in `remove`, to be
/// asked about. Returns whether a value changed, its head (its name and words, held to lift the
/// card) and where it is.
#[allow(clippy::too_many_arguments)]
fn plugin_card(
    ui: &mut Ui,
    plugin: &PluginInfo,
    g: usize,
    plugins: &Plugins,
    moved: &mut Option<(String, f32)>,
    action: &mut Option<String>,
    choice: &mut Option<PluginChoice>,
    textures: &mut Option<String>,
    remove: &mut Option<PluginInfo>,
) -> (bool, egui::Response, Rect) {
    let width = ui.available_width();
    let (area, _) = ui.allocate_exact_size(vec2(width, plugin_height(ui.painter(), plugin, width)), Sense::hover());
    let painter = ui.painter().clone();
    painter.rect_filled(area, 22, argb(ACTIONS_GROUP));
    let (left, name_y) = (area.left() + PLUGIN_INSET, area.top() + 12.0);
    text(&painter, pos2(left, name_y), Align2::LEFT_TOP, &plugin.name, 17.0, Weight::SemiBold, argb(PANEL_TITLE));
    let inside = width - 2.0 * PLUGIN_INSET;
    let said = wrapped(&painter, &plugin.description, 14.0, Weight::Regular, argb(ACTIONS_ROW), inside);
    let said_height = said.size().y;
    painter.galley(pos2(left, area.top() + PLUGIN_HEAD), said, argb(ACTIONS_ROW));
    let mut y = area.top() + PLUGIN_HEAD + said_height;
    if let Some(problem) = &plugin.problem {
        let at = pos2(left, y + PLUGIN_PROBLEM / 2.0 + 2.0);
        text(&painter, at, Align2::LEFT_CENTER, problem, 14.0, Weight::Medium, argb(SLIDE_VALUE));
        y += PLUGIN_PROBLEM;
    }
    y += PLUGIN_FOOT;
    let head_rect = Rect::from_min_max(area.min, pos2(area.right(), y));
    let head = ui.interact(head_rect, ui.scope_id().with(("plugin head", g)), Sense::click());
    let line = |y: f32| Rect::from_min_size(pos2(area.left(), y), vec2(width, 1.0));
    let mut changed = false;
    if controls_show(plugin) && !plugin.features.is_empty() {
        // made of features: each a heading with its switch, what comes under it while it is on
        for (f, feature) in plugin.features.iter().enumerate() {
            painter.rect_filled(line(y), 0, argb(ACTIONS_SEPARATOR));
            y += 1.0;
            let tall = feature_head_height(&painter, feature, width);
            let cell = Rect::from_min_size(pos2(area.left(), y), vec2(width, tall));
            let switch = &feature.control;
            let name_at = pos2(cell.left() + 16.0, cell.top() + FEATURE_PAD);
            text(&painter, name_at, Align2::LEFT_TOP, &switch.label, 17.0, Weight::SemiBold, argb(PANEL_TITLE));
            if !feature.description.is_empty() {
                let said = wrapped(
                    &painter,
                    &feature.description,
                    14.0,
                    Weight::Regular,
                    argb(ACTIONS_ROW),
                    width - FEATURE_ROOM,
                );
                painter.galley(pos2(cell.left() + 16.0, name_at.y + FEATURE_NAME + 4.0), said, argb(ACTIONS_ROW));
            }
            let switch_at = pos2(cell.right() - 8.0 - 63.0, cell.center().y - 14.0);
            crate::transform::switch(ui, switch_at, switch.on, &switch.key);
            if ui.interact(cell, ui.scope_id().with(("plugin feature", g, f)), Sense::click()).clicked() {
                changed |= plugins.set_switch(switch.key.clone(), !switch.on);
            }
            y += tall;
            if !switch.on {
                continue;
            }
            for (i, control) in feature.controls.iter().enumerate() {
                painter.rect_filled(line(y), 0, argb(ACTIONS_SEPARATOR));
                y += 1.0;
                let at = (g, (f + 1) * 100 + i);
                changed |= card_control(ui, &painter, area, &mut y, at, control, plugins, moved, choice, textures);
            }
            for (a, act) in feature.actions.iter().enumerate() {
                painter.rect_filled(line(y), 0, argb(ACTIONS_SEPARATOR));
                let cell = Rect::from_min_size(pos2(area.left(), y + 1.0), vec2(width, ROW_HEIGHT));
                let at = pos2(cell.left() + 16.0, cell.center().y);
                text(&painter, at, Align2::LEFT_CENTER, &act.label, 17.0, Weight::Regular, argb(pc::ACCENT));
                if ui.interact(cell, ui.scope_id().with(("plugin feature action", g, f, a)), Sense::click()).clicked() {
                    *action = Some(act.key.clone());
                }
                y += 1.0 + ROW_HEIGHT;
            }
        }
    } else {
        for (i, control) in plugin.controls.iter().enumerate().filter(|_| controls_show(plugin)) {
            painter.rect_filled(line(y), 0, argb(ACTIONS_SEPARATOR));
            y += 1.0;
            changed |= card_control(ui, &painter, area, &mut y, (g, i), control, plugins, moved, choice, textures);
        }
    }
    for (r, row) in card_rows(plugin).into_iter().enumerate() {
        painter.rect_filled(line(y), 0, argb(ACTIONS_SEPARATOR));
        let cell = Rect::from_min_size(pos2(area.left(), y + 1.0), vec2(width, ROW_HEIGHT));
        let (label, ink) = match row {
            CardRow::Action(i) => (plugin.actions.get(i).map_or("", |a| a.label.as_str()), argb(pc::ACCENT)),
            CardRow::TurnOn => ("Turn on", argb(ACTIONS_ROW)),
            CardRow::TurnOff => ("Turn off", argb(ACTIONS_ROW)),
            CardRow::Remove => ("Remove", argb(ACTIONS_ROW)),
        };
        let at = pos2(cell.left() + 16.0, cell.center().y);
        text(&painter, at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, ink);
        if ui.interact(cell, ui.scope_id().with(("plugin action", g, r)), Sense::click()).clicked() {
            match row {
                CardRow::Action(i) => *action = plugin.actions.get(i).map(|a| a.key.clone()),
                CardRow::TurnOn | CardRow::TurnOff => {
                    changed |= plugins.set_on(plugin.id.clone(), !plugin.on);
                }
                CardRow::Remove => *remove = Some(plugin.clone()),
            }
        }
        y += 1.0 + ROW_HEIGHT;
    }
    (changed, head, area)
}

/// One of a plugin card's controls, whatever it is, drawn at `y` (moved past it), `at` its card's
/// place and its own for what it is told, as [`plugin_card`] has it: whether a value changed.
#[allow(clippy::too_many_arguments)]
fn card_control(
    ui: &mut Ui,
    painter: &Painter,
    area: Rect,
    y: &mut f32,
    (g, i): (usize, usize),
    control: &PluginControl,
    plugins: &Plugins,
    moved: &mut Option<(String, f32)>,
    choice: &mut Option<PluginChoice>,
    textures: &mut Option<String>,
) -> bool {
    let width = area.width();
    let mut changed = false;
    match control {
        PluginControl::Switch { control } => {
            let cell = Rect::from_min_size(pos2(area.left(), *y), vec2(width, ROW_HEIGHT));
            let at = pos2(cell.left() + 16.0, cell.center().y);
            text(painter, at, Align2::LEFT_CENTER, &control.label, 17.0, Weight::Regular, argb(ACTIONS_ROW));
            let switch_at = pos2(cell.right() - 8.0 - 63.0, cell.center().y - 14.0);
            crate::transform::switch(ui, switch_at, control.on, &control.key);
            if ui.interact(cell, ui.scope_id().with(("plugin switch", g, i)), Sense::click()).clicked() {
                changed |= plugins.set_switch(control.key.clone(), !control.on);
            }
            *y += ROW_HEIGHT;
        }
        PluginControl::Slider { control } if !plugin_slider_choices(control.clone()).is_empty() => {
            // its label, the one chosen in blue; a click opens a menu of them
            let cell = Rect::from_min_size(pos2(area.left(), *y), vec2(width, ROW_HEIGHT));
            let at = pos2(cell.left() + 16.0, cell.center().y);
            text(painter, at, Align2::LEFT_CENTER, &control.label, 17.0, Weight::Regular, argb(ACTIONS_ROW));
            let right = pos2(cell.right() - 16.0, cell.center().y);
            let shown = plugin_slider_shown(control.clone(), control.value);
            text(painter, right, Align2::RIGHT_CENTER, &shown, 17.0, Weight::Regular, argb(pc::ACCENT));
            let clicked = ui.interact(cell, ui.scope_id().with(("plugin choice", g, i)), Sense::click()).clicked();
            if clicked || choice.as_ref().is_some_and(|c| c.slider.key == control.key) {
                let clicked = clicked || choice.as_ref().is_some_and(|c| c.clicked);
                *choice = Some(PluginChoice { slider: control.clone(), row: cell, clicked });
            }
            *y += ROW_HEIGHT;
        }
        PluginControl::Slider { control } => {
            let cell = Rect::from_min_size(pos2(area.left(), *y), vec2(width, SLIDE_HEIGHT));
            let held = moved.as_ref().filter(|(key, _)| *key == control.key).map(|(_, value)| *value);
            let value = held.unwrap_or(control.value);
            let shown = plugin_slider_shown(control.clone(), value);
            let along = plugin_slider_along(control.clone(), value);
            if let Some(at) = slide_row(ui, cell, &control.label, &shown, along, (g, i)) {
                let to = plugin_slider_at(control.clone(), at);
                if control.live {
                    changed |= plugins.set_slider(control.key.clone(), to).is_some();
                } else {
                    *moved = Some((control.key.clone(), to));
                }
            }
            *y += SLIDE_HEIGHT;
        }
        PluginControl::Texture { control } => {
            // its label, the texture chosen in blue; a click opens the Textures sheet
            let cell = Rect::from_min_size(pos2(area.left(), *y), vec2(width, ROW_HEIGHT));
            let at = pos2(cell.left() + 16.0, cell.center().y);
            text(painter, at, Align2::LEFT_CENTER, &control.label, 17.0, Weight::Regular, argb(ACTIONS_ROW));
            let right = pos2(cell.right() - 16.0, cell.center().y);
            let shown = crate::textures::chosen_name(plugins, control);
            text(painter, right, Align2::RIGHT_CENTER, &shown, 17.0, Weight::Regular, argb(pc::ACCENT));
            if ui.interact(cell, ui.scope_id().with(("plugin texture", g, i)), Sense::click()).clicked() {
                *textures = Some(control.key.clone());
            }
            *y += ROW_HEIGHT;
        }
        PluginControl::File { control } => {
            // its label; the name of what was picked and Clear, or Import; a click picks another
            let cell = Rect::from_min_size(pos2(area.left(), *y), vec2(width, ROW_HEIGHT));
            let at = pos2(cell.left() + 16.0, cell.center().y);
            text(painter, at, Align2::LEFT_CENTER, &control.label, 17.0, Weight::Regular, argb(ACTIONS_ROW));
            let right = pos2(cell.right() - 16.0, cell.center().y);
            let clear = control.chosen.as_ref().map(|chosen| {
                let clear =
                    text(painter, right, Align2::RIGHT_CENTER, "Clear", 17.0, Weight::Regular, argb(pc::ACCENT));
                let shown: String = if chosen.chars().count() > 28 {
                    chosen.chars().take(27).chain(['\u{2026}']).collect()
                } else {
                    chosen.clone()
                };
                let name_at = pos2(clear.left() - 12.0, cell.center().y);
                text(painter, name_at, Align2::RIGHT_CENTER, &shown, 15.0, Weight::Regular, argb(SLIDE_VALUE));
                clear
            });
            if clear.is_none() {
                text(painter, right, Align2::RIGHT_CENTER, "Import", 17.0, Weight::Regular, argb(pc::ACCENT));
            }
            let response = ui.interact(cell, ui.scope_id().with(("plugin file", g, i)), Sense::click());
            if response.clicked() {
                let on_clear =
                    clear.zip(response.interact_pointer_pos()).is_some_and(|(c, at)| c.expand(8.0).contains(at));
                if on_clear {
                    changed |= plugins.clear_file(control.key.clone());
                } else if control.accepts == procraft_core::PluginFileKind::Picture {
                    if let Some((bytes, name)) = crate::files::pick_control_picture(&control.label) {
                        if plugins.set_picture_file(control.key.clone(), name, bytes) {
                            changed = true;
                        } else {
                            crate::files::say("This picture couldn't be used. Pick a PNG or a JPEG.");
                        }
                    }
                } else if let Some((bytes, name)) = crate::files::pick_sound(&control.label) {
                    if plugins.set_sound_file(control.key.clone(), name, bytes) {
                        changed = true;
                    } else {
                        crate::files::say("This sound couldn't be used. On the computer, pick a WAV file.");
                    }
                }
            }
            *y += ROW_HEIGHT;
        }
    }
    changed
}

/// What a plugin's panel was asked.
#[derive(Clone, Debug, PartialEq)]
pub enum PanelTouch {
    /// Row `.0` touched with `.1` (a slider's, a switch's 1 or 0, a choice's place, 1 for a button).
    Touch(u32, f32),
    /// A row of the plugin's texture control kept under `.0` clicked: its Textures sheet opens.
    Textures(String),
    Close,
}

/// How wide a plugin's panel is, and how far in from the window's right and below the bar.
const PANEL_WIDTH: f32 = 340.0;
const PANEL_MARGIN: f32 = 12.0;

/// A slider value as its row shows it: whole with whole steps, else to two places, then its unit.
fn panel_value(value: f32, step: f32, unit: &str) -> String {
    let whole = step >= 1.0 && step.fract() == 0.0;
    if whole { format!("{}{unit}", value.round() as i64) } else { format!("{value:.2}{unit}") }
}

/// The height of `row` in a plugin's panel, `inside` wide.
/// How far apart a plugin panel's colours are, centre to centre.
const SWATCH_STEP: f32 = 30.0;

fn panel_row_height(painter: &Painter, row: &PanelRow, inside: f32) -> f32 {
    match row {
        PanelRow::Text { text } => {
            wrapped(painter, text, 15.0, Weight::Regular, Color32::WHITE, inside - 32.0).size().y + 22.0
        }
        PanelRow::Slider { .. } => SLIDE_HEIGHT,
        // its label, then its brushes under it
        PanelRow::Brushes { .. } => BRUSHES_ROW_HEIGHT,
        _ => ROW_HEIGHT,
    }
}

/// A plugin panel's row of brushes: how tall, how big each brush's disc is and how far apart.
const BRUSHES_ROW_HEIGHT: f32 = 86.0;
const BRUSH_DISC: f32 = 34.0;
const BRUSH_STEP: f32 = 38.0;

/// The sample strokes a plugin's panel shows its brushes with, by their place in the library, as
/// drawn so far.
pub type Strokes = HashMap<(u32, u32), egui::TextureId>;

/// A brush on a plugin's panel or ring: its sample stroke (`stroke`, once drawn) in a disc of
/// `radius` about `centre`, white on the accent while `chosen`.
fn brush_disc(painter: &Painter, centre: Pos2, radius: f32, stroke: Option<egui::TextureId>, chosen: bool) {
    painter.circle_filled(centre, radius, if chosen { argb(pc::ACCENT) } else { argb(pc::ACTION_DISC) });
    if let Some(stroke) = stroke {
        // the stroke's middle part across the disc, in the square inside it
        let side = radius * std::f32::consts::SQRT_2;
        let shown = Rect::from_center_size(centre, vec2(side, side));
        let uv = Rect::from_min_max(pos2(0.3, 0.0), pos2(0.7, 1.0));
        let tint = if crate::look::light() && !chosen { Color32::BLACK } else { Color32::WHITE };
        painter.image(stroke, shown, uv, tint);
    }
    let edge = if chosen { Stroke::new(2.0, argb(pc::ACCENT)) } else { Stroke::new(1.0, argb(RING_EDGE)) };
    painter.circle_stroke(centre, radius, edge);
}

const RING_EDGE: Themed = Themed(0xFF5A5A5A, 0xFFC0C0C0);

/// How far a ring's first row runs from its middle and each next one further, a colour's disc
/// and a brush's across, and how far its backdrop reaches past its outer row.
const RING_FIRST: f32 = 62.0;
const RING_STEP: f32 = 58.0;
const RING_COLOUR: f32 = 34.0;
const RING_BRUSH: f32 = 52.0;
const RING_MARGIN: f32 = 12.0;

/// How far a ring of `rows` reaches from its middle, its backdrop with it.
fn ring_reach(rows: &[PanelRow]) -> f32 {
    let outer = rows.len().saturating_sub(1) as f32;
    let item = if matches!(rows.last(), Some(PanelRow::Brushes { .. })) { RING_BRUSH } else { RING_COLOUR };
    RING_FIRST + RING_STEP * outer + item / 2.0 + RING_MARGIN
}

/// Where item `i` of `n` sits on row `k` of a ring about `middle`, the first at the top and the
/// rest clockwise, and how big it is (`base` at most): they never touch, 6 apart at least.
fn ring_item(middle: Pos2, k: usize, i: usize, n: usize, base: f32) -> (Pos2, f32) {
    let radius = RING_FIRST + RING_STEP * k as f32;
    let n = n.max(1);
    let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / n as f32;
    // as far apart as their middles are, less 6
    let size = if n == 1 { base } else { base.min(2.0 * radius * (std::f32::consts::PI / n as f32).sin() - 6.0) };
    (middle + vec2(angle.cos(), angle.sin()) * radius, size)
}

/// A plugin's panel shown as a ring (interface 11) about where its action came from (`panel.at`,
/// window pixels; else the window's middle), kept in the window where it fits: its rows of colours
/// and brushes each a ring, the first innermost, on a round backdrop. A tap on a colour or a brush
/// is told as its place; one off the backdrop closes it.
pub fn plugin_ring(ctx: &egui::Context, panel: &PluginPanel, strokes: &Strokes) -> Option<PanelTouch> {
    let screen = ctx.content_rect();
    let reach = ring_reach(&panel.rows);
    let want = panel.at.map_or(screen.center(), |at| pos2(at.x, at.y) / ctx.pixels_per_point());
    let inside = |v: f32, least: f32, most: f32| {
        if most - least < reach * 2.0 { (least + most) / 2.0 } else { v.clamp(least + reach, most - reach) }
    };
    let middle = pos2(inside(want.x, screen.left(), screen.right()), inside(want.y, screen.top(), screen.bottom()));
    let mut asked = None;
    egui::Area::new(Id::unique("plugin ring")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
        let around = ui.allocate_rect(screen, Sense::click());
        let painter = ui.painter().clone();
        painter.circle_filled(middle, reach, argb(panel::POPOVER_FROST));
        painter.circle_stroke(middle, 4.0, Stroke::new(1.5, argb(PANEL_TITLE)));
        for (k, row) in panel.rows.iter().enumerate() {
            let (n, base, chosen) = match row {
                PanelRow::Colours { colours, chosen, .. } => (colours.len(), RING_COLOUR, *chosen),
                PanelRow::Brushes { brushes, chosen, .. } => (brushes.len(), RING_BRUSH, *chosen),
                _ => continue,
            };
            for i in 0..n {
                let (centre, size) = ring_item(middle, k, i, n, base);
                let picked = chosen == Some(i as u32);
                match row {
                    PanelRow::Colours { colours, .. } => {
                        let [_, r, g, b] = colours.get(i).copied().unwrap_or(0).to_be_bytes();
                        let fill = Color32::from_rgb(r, g, b);
                        if picked {
                            painter.circle_stroke(centre, size / 2.0 - 1.0, Stroke::new(2.0, argb(pc::TEXT)));
                            painter.circle_filled(centre, size / 2.0 - 5.0, fill);
                        } else {
                            painter.circle_filled(centre, size / 2.0, fill);
                        }
                    }
                    PanelRow::Brushes { brushes, .. } => {
                        let stroke = brushes.get(i).and_then(|b| strokes.get(&(b.set, b.index)).copied());
                        brush_disc(&painter, centre, size / 2.0, stroke, picked);
                    }
                    _ => {}
                }
                let spot = Rect::from_center_size(centre, vec2(size, size));
                if ui.interact(spot, ui.scope_id().with(("ring item", k, i)), Sense::click()).clicked() {
                    asked = Some(PanelTouch::Touch(k as u32, i as f32));
                }
            }
        }
        // off the backdrop: closed, as Done would
        let off = around.interact_pointer_pos().is_some_and(|p| p.distance(middle) > reach);
        if asked.is_none() && around.clicked() && off {
            asked = Some(PanelTouch::Close);
        }
    });
    asked
}

/// A plugin's panel, floating under the bar at the window's right: its title and Done, then its rows
/// on a card as the Actions panel's are. A slider moved is told as it lets go (`held` keeps where it
/// is meanwhile); anything else at once. Returns what was asked. It shows at the size a plugin
/// gives floating panels, its top right corner where it stays.
#[allow(clippy::too_many_arguments)]
pub fn plugin_panel(
    ctx: &egui::Context,
    look: &mut Look,
    plugins: &Plugins,
    panel: &PluginPanel,
    held: &mut Option<(usize, f32)>,
    backdrops: &mut Vec<f32>,
    strokes: &Strokes,
) -> Option<PanelTouch> {
    let screen = ctx.content_rect();
    let k = panel::part_scale(ctx, procraft_core::InterfacePart::Floating);
    let top = screen.top() + panel::bar_shown(ctx) + PANEL_MARGIN;
    let left = screen.right() - PANEL_MARGIN - PANEL_WIDTH * k;
    let inside = PANEL_WIDTH - 20.0;
    let painter = ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::unique("plugin panel measure")));
    let rows_height: f32 = panel.rows.iter().map(|r| panel_row_height(&painter, r, inside)).sum::<f32>()
        + panel.rows.len().saturating_sub(1) as f32;
    let height = (52.0 + 8.0 + rows_height + 10.0).min((screen.bottom() - PANEL_MARGIN - top) / k);
    let rect = Rect::from_min_size(pos2(left, top), vec2(PANEL_WIDTH, height));
    let mut asked = None;
    // laid out at its own size, and drawn (and touched) at its scale from its top left, kept to the
    // window and clipped to it as the window measures it at that scale
    let area = Id::unique("plugin panel");
    let to_window = crate::chrome::scaled_from(rect.min, k);
    ctx.set_transform_layer(egui::LayerId::new(Order::Foreground, area), to_window);
    let measured = to_window.inverse().mul_rect(screen);
    egui::Area::new(area).fixed_pos(rect.min).order(Order::Foreground).constrain_to(measured).show(ctx, |ui| {
        ui.allocate_rect(rect, Sense::click());
        panel::floating_frame(ui, look, "plugin panel frame", rect, k, backdrops);
        let painter = ui.painter().clone();
        let title_at = pos2(rect.left() + 24.0, rect.top() + 22.5);
        text(&painter, title_at, Align2::LEFT_TOP, &panel.title, 22.0, Weight::Bold, argb(crate::panel::PANEL_TITLE));
        let done_at = pos2(rect.right() - 24.0, rect.top() + 33.0);
        let done = text(&painter, done_at, Align2::RIGHT_CENTER, "Done", 17.0, Weight::Regular, argb(pc::ACCENT));
        if ui.interact(done.expand(8.0), ui.scope_id().with("plugin panel done"), Sense::click()).clicked() {
            asked = Some(PanelTouch::Close);
        }
        let list = Rect::from_min_max(pos2(rect.left() + 10.0, rect.top() + 52.0 + 8.0), rect.max - vec2(10.0, 10.0));
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            ScrollArea::vertical()
                .id_salt("plugin panel rows")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    let card_height = rows_height;
                    let (card, _) = ui.allocate_exact_size(vec2(list.width(), card_height), Sense::hover());
                    let painter = ui.painter().clone();
                    painter.rect_filled(card, 13, argb(ACTIONS_GROUP));
                    let mut y = card.top();
                    for (i, row) in panel.rows.iter().enumerate() {
                        if i > 0 {
                            let line = Rect::from_min_size(pos2(card.left(), y), vec2(card.width(), 1.0));
                            painter.rect_filled(line, 0, argb(ACTIONS_SEPARATOR));
                            y += 1.0;
                        }
                        let h = panel_row_height(&painter, row, inside);
                        let cell = Rect::from_min_size(pos2(card.left(), y), vec2(card.width(), h));
                        let label_at = pos2(cell.left() + 16.0, cell.center().y);
                        let tapped = |ui: &Ui| {
                            ui.interact(cell, ui.scope_id().with(("plugin row", i)), Sense::click()).clicked()
                        };
                        let row_at = i as u32;
                        match row {
                            PanelRow::Text { text: words } => {
                                let said = wrapped(
                                    &painter,
                                    words,
                                    15.0,
                                    Weight::Regular,
                                    argb(ACTIONS_ROW),
                                    cell.width() - 32.0,
                                );
                                painter.galley(pos2(cell.left() + 16.0, cell.top() + 11.0), said, argb(ACTIONS_ROW));
                            }
                            PanelRow::Slider { label, least, most, step, value, unit } => {
                                let shown = held.filter(|(at, _)| *at == i).map_or(*value, |(_, v)| v);
                                let along = (shown - least) / (most - least);
                                let words = panel_value(shown, *step, unit);
                                if let Some(at) = slide_row(ui, cell, label, &words, along, (usize::MAX, i)) {
                                    let to = least + at * (most - least);
                                    let to =
                                        if *step > 0.0 { least + ((to - least) / step).round() * step } else { to };
                                    *held = Some((i, to.clamp(*least, *most)));
                                }
                            }
                            PanelRow::Switch { label, on } => {
                                text(
                                    &painter,
                                    label_at,
                                    Align2::LEFT_CENTER,
                                    label,
                                    17.0,
                                    Weight::Regular,
                                    argb(ACTIONS_ROW),
                                );
                                let switch_at = pos2(cell.right() - 8.0 - 63.0, cell.center().y - 14.0);
                                crate::transform::switch(ui, switch_at, *on, ("plugin panel switch", i));
                                if tapped(ui) {
                                    asked = Some(PanelTouch::Touch(row_at, if *on { 0.0 } else { 1.0 }));
                                }
                            }
                            PanelRow::Choice { label, options, chosen } => {
                                text(
                                    &painter,
                                    label_at,
                                    Align2::LEFT_CENTER,
                                    label,
                                    17.0,
                                    Weight::Regular,
                                    argb(ACTIONS_ROW),
                                );
                                let picked = options.get(*chosen as usize).map_or("", String::as_str);
                                let right = pos2(cell.right() - 16.0, cell.center().y);
                                text(
                                    &painter,
                                    right,
                                    Align2::RIGHT_CENTER,
                                    picked,
                                    17.0,
                                    Weight::Regular,
                                    argb(pc::ACCENT),
                                );
                                if tapped(ui) {
                                    let next = (*chosen as usize + 1) % options.len().max(1);
                                    asked = Some(PanelTouch::Touch(row_at, next as f32));
                                }
                            }
                            PanelRow::Button { label } => {
                                text(
                                    &painter,
                                    label_at,
                                    Align2::LEFT_CENTER,
                                    label,
                                    17.0,
                                    Weight::Regular,
                                    argb(pc::ACCENT),
                                );
                                if tapped(ui) {
                                    asked = Some(PanelTouch::Touch(row_at, 1.0));
                                }
                            }
                            // its label, then each colour a disc from the row's right end, the
                            // one chosen ringed
                            PanelRow::Colours { label, colours, chosen } => {
                                text(
                                    &painter,
                                    label_at,
                                    Align2::LEFT_CENTER,
                                    label,
                                    17.0,
                                    Weight::Regular,
                                    argb(ACTIONS_ROW),
                                );
                                let count = colours.len();
                                for (at, &colour) in colours.iter().enumerate() {
                                    let x =
                                        cell.right() - 10.0 - SWATCH_STEP / 2.0 - (count - 1 - at) as f32 * SWATCH_STEP;
                                    let centre = pos2(x, cell.center().y);
                                    let [_, r, g, b] = colour.to_be_bytes();
                                    let fill = Color32::from_rgb(r, g, b);
                                    if *chosen == Some(at as u32) {
                                        painter.circle_stroke(centre, 11.0, egui::Stroke::new(2.0, argb(ACTIONS_ROW)));
                                        painter.circle_filled(centre, 8.0, fill);
                                    } else {
                                        painter.circle_filled(centre, 12.0, fill);
                                    }
                                    let spot = Rect::from_center_size(centre, vec2(SWATCH_STEP, cell.height()));
                                    let id = ui.scope_id().with(("plugin colour", i, at));
                                    if ui.interact(spot, id, Sense::click()).clicked() {
                                        asked = Some(PanelTouch::Touch(row_at, at as f32));
                                    }
                                }
                            }
                            // its label, then each brush's stroke in a disc under it, the one
                            // chosen on the accent
                            PanelRow::Brushes { label, brushes, chosen } => {
                                let label_at = pos2(cell.left() + 16.0, cell.top() + 22.0);
                                text(
                                    &painter,
                                    label_at,
                                    Align2::LEFT_CENTER,
                                    label,
                                    17.0,
                                    Weight::Regular,
                                    argb(ACTIONS_ROW),
                                );
                                for (at, b) in brushes.iter().enumerate() {
                                    let x = cell.left() + 16.0 + BRUSH_DISC / 2.0 + at as f32 * BRUSH_STEP;
                                    let centre = pos2(x, cell.top() + 44.0 + BRUSH_DISC / 2.0);
                                    let stroke = strokes.get(&(b.set, b.index)).copied();
                                    brush_disc(&painter, centre, BRUSH_DISC / 2.0, stroke, *chosen == Some(at as u32));
                                    let spot = Rect::from_center_size(centre, vec2(BRUSH_STEP, BRUSH_STEP));
                                    let id = ui.scope_id().with(("plugin brush", i, at));
                                    if ui.interact(spot, id, Sense::click()).clicked() {
                                        asked = Some(PanelTouch::Touch(row_at, at as f32));
                                    }
                                }
                            }
                            // its label, and the keys at its end as a file control's name is
                            PanelRow::Keys { label, keys } => {
                                text(
                                    &painter,
                                    label_at,
                                    Align2::LEFT_CENTER,
                                    label,
                                    17.0,
                                    Weight::Regular,
                                    argb(ACTIONS_ROW),
                                );
                                let right = pos2(cell.right() - 16.0, cell.center().y);
                                text(
                                    &painter,
                                    right,
                                    Align2::RIGHT_CENTER,
                                    keys,
                                    15.0,
                                    Weight::Regular,
                                    argb(SLIDE_VALUE),
                                );
                                if tapped(ui) {
                                    asked = Some(PanelTouch::Touch(row_at, 1.0));
                                }
                            }
                            // one of its texture controls, as its card shows it (others show nothing)
                            PanelRow::Control { key } => {
                                let texture =
                                    plugins.plugins().into_iter().flat_map(|p| p.controls).find_map(|c| match c {
                                        PluginControl::Texture { control } if control.key == *key => Some(control),
                                        _ => None,
                                    });
                                if let Some(control) = texture {
                                    let ink = argb(ACTIONS_ROW);
                                    text(
                                        &painter,
                                        label_at,
                                        Align2::LEFT_CENTER,
                                        &control.label,
                                        17.0,
                                        Weight::Regular,
                                        ink,
                                    );
                                    let right = pos2(cell.right() - 16.0, cell.center().y);
                                    let shown = crate::textures::chosen_name(plugins, &control);
                                    text(
                                        &painter,
                                        right,
                                        Align2::RIGHT_CENTER,
                                        &shown,
                                        17.0,
                                        Weight::Regular,
                                        argb(pc::ACCENT),
                                    );
                                    if tapped(ui) {
                                        asked = Some(PanelTouch::Textures(key.clone()));
                                    }
                                }
                            }
                        }
                        y += h;
                    }
                });
        });
    });
    // a slider let go is told where it went
    if !ctx.input(|i| i.pointer.any_down())
        && let Some((row, value)) = held.take()
    {
        asked = asked.take().or(Some(PanelTouch::Touch(row as u32, value)));
    }
    asked
}

/// A small menu hanging beside `row`: `labels`, each with a round mark, `chosen`'s ticked; 254
/// wide, its left edge 54 in from the row's right end, kept on the screen. Returns the one picked.
#[allow(clippy::too_many_arguments)]
fn choice_menu(
    menu: &mut Menu,
    ctx: &egui::Context,
    look: &mut Look,
    backdrops: &mut Vec<f32>,
    id: &str,
    row: Rect,
    labels: &[&str],
    chosen: usize,
) -> Option<usize> {
    let height = 8.0 + 40.0 * labels.len() as f32 + 8.0;
    let screen = panel::screen(ctx);
    let top = (row.top() - 8.0).min(screen.bottom() - 8.0 - height).max(screen.top() + 8.0);
    let rect = Rect::from_min_size(pos2(row.right() - 54.0, top), vec2(254.0, height));
    let mut picked = None;
    menu.show(ctx, id, rect, rect.min, CHOICE_MENU, backdrops, |ui| {
        for (i, label) in labels.iter().enumerate() {
            let line =
                Rect::from_min_size(pos2(rect.left(), rect.top() + 8.0 + 40.0 * i as f32), vec2(rect.width(), 40.0));
            let at = pos2(line.left() + 20.0, line.center().y);
            text(ui.painter(), at, Align2::LEFT_CENTER, label, 14.5, Weight::Regular, argb(pc::TEXT));
            radio(ui, look, pos2(line.right() - 19.0 - 9.0, line.center().y), i == chosen);
            if ui.interact(line, ui.scope_id().with((id, i)), Sense::click()).clicked() {
                picked = Some(i);
            }
        }
    });
    picked
}

/// A radio mark centred at `at`: blue with a tick when chosen, else a grey ring.
pub(crate) fn radio(ui: &Ui, look: &mut Look, at: Pos2, chosen: bool) {
    if chosen {
        ui.painter().circle_filled(at, 9.0, argb(pc::ACCENT));
        look.icon(ui, &icons::TICK, at, 12.0, Color32::WHITE);
    } else {
        ui.painter().circle_stroke(at, 9.0 - 0.75, Stroke::new(1.5, argb(RADIO_RING)));
    }
}

/// Choices as rows of a rounded group in `rect`, the chosen one a full blue row. Returns the one
/// tapped.
fn choice_group(ui: &Ui, rect: Rect, labels: &[&str], chosen: usize, g: usize) -> Option<usize> {
    ui.painter().rect_filled(rect, 18, argb(ACTIONS_GROUP));
    let mut tapped = None;
    let mut y = rect.top();
    for (i, label) in labels.iter().enumerate() {
        if i > 0 && i != chosen && i - 1 != chosen {
            let line = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 1.0));
            ui.painter().rect_filled(line, 0, argb(ACTIONS_SEPARATOR));
            y += 1.0;
        }
        let row = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), CHOICE_HEIGHT));
        if i == chosen {
            let corner = |at: bool| if at { 18 } else { 0 };
            let corners = egui::CornerRadius {
                nw: corner(i == 0),
                ne: corner(i == 0),
                sw: corner(i + 1 == labels.len()),
                se: corner(i + 1 == labels.len()),
            };
            ui.painter().rect_filled(row, corners, argb(pc::ACCENT));
        }
        let ink = if i == chosen { Color32::WHITE } else { argb(PANEL_TITLE) };
        let at = pos2(row.left() + 16.0, row.center().y);
        text(ui.painter(), at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, ink);
        if ui.interact(row, ui.scope_id().with(("choice", g, i)), Sense::click()).clicked() {
            tapped = Some(i);
        }
        y += CHOICE_HEIGHT;
    }
    tapped
}

/// A group: led by its heading (above it, not a row of it), its rows on a dark rounded wash with
/// faint lines between them (none between sliders). Returns what was tapped, and where each row
/// is.
fn group_view(ui: &mut Ui, rows: &[Row], g: usize, swipes: &mut Swipes) -> (Option<Tapped>, Vec<Rect>) {
    let width = ui.available_width();
    let mut rows = rows.iter().peekable();
    if let Some(Row::Heading(label)) = rows.peek() {
        let (band, _) = ui.allocate_exact_size(vec2(width, HEADING_HEIGHT), Sense::hover());
        text(ui.painter(), band.min + vec2(13.0, 7.0), Align2::LEFT_TOP, label, 14.0, Weight::Bold, argb(PANEL_TITLE));
        rows.next();
    }
    let rows: Vec<&Row> = rows.collect();
    let (area, _) = ui.allocate_exact_size(vec2(width, group_height_of(&rows)), Sense::hover());
    ui.painter().rect_filled(area, 22, argb(ACTIONS_GROUP));
    let mut tapped = None;
    let mut cells = Vec::new();
    let mut y = area.top();
    for (i, row) in rows.iter().enumerate() {
        let after_slide = i > 0 && matches!(rows.get(i - 1), Some(Row::Slide(..)));
        if i > 0 && !(after_slide && matches!(row, Row::Slide(..))) {
            let line = Rect::from_min_size(pos2(area.left(), y), vec2(width, 1.0));
            ui.painter().rect_filled(line, 0, argb(ACTIONS_SEPARATOR));
            y += 1.0;
        }
        let cell = Rect::from_min_size(pos2(area.left(), y), vec2(width, row.height()));
        cells.push(cell);
        // the first and last rows follow the group's rounded corners
        let corner = |at: bool| if at { 22 } else { 0 };
        let corners = egui::CornerRadius {
            nw: corner(i == 0),
            ne: corner(i == 0),
            sw: corner(i + 1 == rows.len()),
            se: corner(i + 1 == rows.len()),
        };
        let label_at = pos2(cell.left() + 16.0, cell.center().y);
        let switch_at = pos2(cell.right() - 8.0 - 63.0, cell.center().y - 14.0);
        match row {
            Row::Button(label, value, act) => {
                let response = ui.interact(cell, ui.scope_id().with(("row", g, i)), Sense::click());
                button_row(ui, &response, cell, corners, label, *value, act.is_some());
                if response.clicked() {
                    tapped = act.map(Tapped::Act);
                }
            }
            Row::Private(label, private, act, private_act) => {
                if let Some(t) = private_row(ui, cell, corners, (label, private), (*act, *private_act), swipes, (g, i))
                {
                    tapped = Some(t);
                }
            }
            Row::Switch(label, on, Some(act)) => {
                text(ui.painter(), label_at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, argb(ACTIONS_ROW));
                crate::transform::switch(ui, switch_at, *on, label);
                if ui.interact(cell, ui.scope_id().with(("switch", g, i)), Sense::click()).clicked() {
                    tapped = Some(Tapped::Act(*act));
                }
            }
            Row::Switch(label, on, None) => {
                text(ui.painter(), label_at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, argb(ACTIONS_DISABLED));
                // a 63 x 28 switch, shown as it is: it can't be changed here
                let track = Rect::from_min_size(switch_at, vec2(63.0, 28.0));
                ui.painter().rect_filled(track, 14, argb(SWITCH_FIXED));
                let knob =
                    Rect::from_min_size(track.min + vec2(2.0 + if *on { 22.0 } else { 0.0 }, 2.0), vec2(37.0, 24.0));
                ui.painter().rect_filled(knob, 12, argb(SWITCH_FIXED_KNOB));
            }
            Row::Slide(label, value, fraction, _, which) => {
                if let Some(at) = slide_row(ui, cell, label, value, *fraction, (g, i)) {
                    tapped = Some(Tapped::Slid(*which, at));
                }
            }
            Row::Heading(_) => {}
        }
        y += row.height();
    }
    (tapped, cells)
}

/// A button row at `cell`: it turns blue while it is held (`response`), its text white; dimmed
/// unless it does something (`acts`). Its value, if any, on the right.
fn button_row(
    ui: &Ui,
    response: &egui::Response,
    cell: Rect,
    corners: egui::CornerRadius,
    label: &str,
    value: Option<&str>,
    acts: bool,
) {
    let held = response.is_pointer_button_down_on() && !response.dragged();
    let lit = if acts { highlight(ui.ctx(), response.id, held) } else { 0.0 };
    if lit > 0.0 {
        ui.painter().rect_filled(cell, corners, argb(pc::ACCENT).gamma_multiply(lit));
    }
    let idle = if acts { argb(ACTIONS_ROW) } else { argb(ACTIONS_DISABLED) };
    let ink = mix(idle, Color32::WHITE, lit);
    text(
        ui.painter(),
        pos2(cell.left() + 16.0, cell.center().y),
        Align2::LEFT_CENTER,
        label,
        17.0,
        Weight::Regular,
        ink,
    );
    if let Some(v) = value {
        let shown = mix(argb(TAB_IDLE), Color32::WHITE, lit);
        let at = pos2(cell.right() - 16.0, cell.center().y);
        text(ui.painter(), at, Align2::RIGHT_CENTER, v, 15.0, Weight::Regular, shown);
    }
}

/// A row with a private action at `cell`: a swipe to the left slides it open (let go past halfway
/// it stays open), its private button coming in from the right; a tap on the row closes it again,
/// one on the button does it privately. Returns what was tapped.
fn private_row(
    ui: &Ui,
    cell: Rect,
    corners: egui::CornerRadius,
    (label, private): (&str, &str),
    (act, private_act): (Option<Act>, Option<Act>),
    swipes: &mut Swipes,
    at: (usize, usize),
) -> Option<Tapped> {
    let now = ui.input(|i| i.time);
    let id = ui.scope_id().with(("private row", at));
    let response = ui.interact(cell, id, Sense::click_and_drag());
    let swipe = swipes.entry(at).or_default();
    if response.drag_started() {
        swipe.dragging = Some(swipe.at(now));
    }
    if let Some(d) = swipe.dragging.as_mut() {
        *d = (*d - response.drag_delta().x).clamp(0.0, PRIVATE_SLIDE);
    }
    if response.drag_stopped() {
        let open = swipe.at(now) > PRIVATE_SLIDE / 2.0;
        swipe.settle(open, now);
    }
    let slide = swipe.at(now);
    if swipe.dragging.is_none() && (now - swipe.since) < f64::from(PRIVATE_SETTLE_S) {
        ui.ctx().request_repaint();
    }
    let painter = ui.painter_at(cell);
    if slide <= 0.0 {
        button_row(ui, &response, cell, corners, label, None, act.is_some());
        return response.clicked().then_some(act).flatten().map(Tapped::Act);
    }
    // slid aside, the row only closes again when tapped
    let ink = if act.is_some() { argb(ACTIONS_ROW) } else { argb(ACTIONS_DISABLED) };
    let label_at = pos2(cell.left() + 16.0 - slide, cell.center().y);
    text(&painter, label_at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, ink);
    let left = cell.right() - PRIVATE_WIDTH + (PRIVATE_SLIDE - slide);
    let button = Rect::from_min_size(pos2(left, cell.top()), vec2(PRIVATE_WIDTH, cell.height()));
    // rounded 13, its outer corners as round as the group's where it meets them
    let round = egui::CornerRadius { nw: 13, sw: 13, ne: corners.ne.max(13), se: corners.se.max(13) };
    painter.rect_filled(button, round, argb(PRIVATE_FILL));
    let ink = if private_act.is_some() { argb(PRIVATE_INK) } else { argb(ACTIONS_DISABLED) };
    text(&painter, button.center(), Align2::CENTER_CENTER, private, 17.0, Weight::Regular, ink);
    let pressed = ui.interact(button.intersect(cell), id.with("private"), Sense::click()).clicked();
    if pressed && private_act.is_some() {
        *swipe = Swipe::default();
        return private_act.map(Tapped::Act);
    }
    if response.clicked() {
        swipe.settle(false, now);
    }
    None
}

/// The rows' height in a group, with the lines between them (none between sliders).
fn group_height_of(rows: &[&Row]) -> f32 {
    rows.iter().map(|r| r.height()).sum::<f32>()
        + rows.windows(2).filter(|w| !matches!(w, [Row::Slide(..), Row::Slide(..)])).count() as f32
}

/// A slider row at `cell`: the name and value on top, the bar under them, blue up to a small white
/// knob; dragged or tapped along, it goes there. Returns where it was set (0..1), while held.
fn slide_row(ui: &Ui, cell: Rect, label: &str, value: &str, fraction: f32, id: (usize, usize)) -> Option<f32> {
    let inset = 16.0;
    let response = ui.interact(cell, ui.scope_id().with(("slide", id)), Sense::click_and_drag());
    let set = (response.is_pointer_button_down_on() || response.clicked())
        .then(|| response.interact_pointer_pos())
        .flatten()
        .map(|p| ((p.x - cell.left() - inset) / (cell.width() - 2.0 * inset)).clamp(0.0, 1.0));
    let painter = ui.painter();
    let (name_at, value_at) = (pos2(cell.left() + 16.0, cell.top() + 9.0), pos2(cell.right() - 19.0, cell.top() + 9.0));
    text(painter, name_at, Align2::LEFT_TOP, label, 17.0, Weight::Regular, argb(SLIDE_LABEL));
    text(painter, value_at, Align2::RIGHT_TOP, value, 17.0, Weight::Regular, argb(SLIDE_VALUE));
    let y = cell.bottom() - 17.5;
    let (from, to) = (cell.left() + inset, cell.right() - inset);
    let x = from + fraction.clamp(0.0, 1.0) * (to - from);
    painter.rect_filled(Rect::from_min_max(pos2(from, y - 1.5), pos2(to, y + 1.5)), 0, argb(SLIDE_TRACK));
    painter.rect_filled(Rect::from_min_max(pos2(from, y - 1.5), pos2(x, y + 1.5)), 0, argb(pc::ACCENT));
    painter.rect_filled(Rect::from_center_size(pos2(x, y), vec2(16.0, 10.0)), 5, Color32::WHITE);
    set
}

#[cfg(test)]
mod tests {
    use egui::{Event, Modifiers, PointerButton, RawInput};
    use procraft_core::{PluginSlider, PluginSwitch};

    use super::*;

    /// What a tap at `at` on a plugin's panel of a button, a switch, a choice and a row of keys asks
    /// (the window 1200 wide).
    fn panel_tapped(at: impl Fn(Rect) -> Pos2) -> Option<PanelTouch> {
        panel_tapped_at(1.0, at)
    }

    /// As [`panel_tapped`], floating panels shown `scale` times as big: `at` where it is laid out,
    /// the tap where that shows.
    fn panel_tapped_at(scale: f32, at: impl Fn(Rect) -> Pos2) -> Option<PanelTouch> {
        let (ctx, mut look) = (egui::Context::default(), Look::default());
        crate::look::install_fonts(&ctx);
        let panel = PluginPanel {
            title: "Squares".to_owned(),
            rows: vec![
                PanelRow::Button { label: "Add a layer".to_owned() },
                PanelRow::Switch { label: "Round".to_owned(), on: true },
                PanelRow::Choice {
                    label: "Shape".to_owned(),
                    options: vec!["Dots".to_owned(), "Lines".to_owned()],
                    chosen: 1,
                },
                PanelRow::Keys { label: "Undo".to_owned(), keys: "Ctrl+Z".to_owned() },
                PanelRow::Colours {
                    label: "Colour".to_owned(),
                    colours: vec![0xFF0000, 0x00FF00, 0x0000FF],
                    chosen: None,
                },
            ],
            key_press: false,
            ring: false,
            at: None,
        };
        let top = pc::BAR_HEIGHT + PANEL_MARGIN;
        let left = 1200.0 - PANEL_MARGIN - PANEL_WIDTH * scale;
        let rect = Rect::from_min_size(pos2(left, top), vec2(PANEL_WIDTH, 300.0));
        let point = crate::chrome::scaled_from(rect.min, scale) * at(rect);
        let (mut time, mut asked, mut held) = (0.0, None, None);
        let press = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        for step in 0..6 {
            time += 1.0 / 60.0;
            let events = match step {
                2 => vec![Event::PointerMoved(point)],
                3 => vec![press(point, true)],
                4 => vec![press(point, false)],
                _ => Vec::new(),
            };
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 900.0))),
                time: Some(time),
                events,
                ..RawInput::default()
            };
            let mut output = ctx.clone().run_ui(raw, |ui| {
                panel::put_part_scales(ui.ctx(), [scale, 1.0, 1.0, 1.0, 1.0]);
                asked = asked.take().or(plugin_panel(
                    ui.ctx(),
                    &mut look,
                    &Plugins::new(),
                    &panel,
                    &mut held,
                    &mut Vec::new(),
                    &Strokes::new(),
                ));
            });
            output.textures_delta.clear();
        }
        asked
    }

    #[test]
    fn a_plugin_panel_s_rows_tell_their_touches_and_done_closes_it() {
        // rows from 60 below the top, 44 tall with a line between
        let row = |i: f32| move |r: Rect| pos2(r.left() + 60.0, r.top() + 60.0 + 22.0 + i * 45.0);
        assert_eq!(panel_tapped(row(0.0)), Some(PanelTouch::Touch(0, 1.0)), "the button");
        assert_eq!(panel_tapped(row(1.0)), Some(PanelTouch::Touch(1, 0.0)), "the switch turned off");
        assert_eq!(panel_tapped(row(2.0)), Some(PanelTouch::Touch(2, 0.0)), "the choice to the next, round");
        assert_eq!(panel_tapped(row(3.0)), Some(PanelTouch::Touch(3, 1.0)), "the keys");
        // the colours from the row's right end, each its place; the label between them none
        let colour =
            |from_right: f32| move |r: Rect| pos2(r.right() - 25.0 - from_right * 30.0, r.top() + 82.0 + 4.0 * 45.0);
        assert_eq!(panel_tapped(colour(0.0)), Some(PanelTouch::Touch(4, 2.0)), "the last colour");
        assert_eq!(panel_tapped(colour(2.0)), Some(PanelTouch::Touch(4, 0.0)), "the first colour");
        assert_eq!(panel_tapped(row(4.0)), None, "the colours' label");
        assert_eq!(panel_tapped(|r| pos2(r.right() - 40.0, r.top() + 33.0)), Some(PanelTouch::Close));
        assert_eq!(panel_tapped(|r| pos2(r.left() + 60.0, r.top() + 20.0)), None, "the title does nothing");
    }

    #[test]
    fn a_pen_s_tap_on_a_plugin_panel_row_reaches_it_as_a_touch_with_its_wobble() {
        // the system's pen: its touches, which egui-winit also makes its pointer's, and the moves
        // a pen makes between press and lift without going anywhere
        let row = |r: Rect| pos2(r.left() + 60.0, r.top() + 60.0 + 22.0 + 45.0);
        let (ctx, mut look) = (egui::Context::default(), Look::default());
        crate::look::install_fonts(&ctx);
        let panel = PluginPanel {
            title: "Squares".to_owned(),
            rows: vec![
                PanelRow::Button { label: "Add a layer".to_owned() },
                PanelRow::Switch { label: "Round".to_owned(), on: true },
            ],
            key_press: false,
            ring: false,
            at: None,
        };
        let top = pc::BAR_HEIGHT + PANEL_MARGIN;
        let rect = Rect::from_min_size(pos2(1200.0 - PANEL_MARGIN - PANEL_WIDTH, top), vec2(PANEL_WIDTH, 300.0));
        let at = row(rect);
        let touch = |phase, pos| Event::Touch {
            device_id: egui::TouchDeviceId(1),
            id: egui::TouchId(1),
            phase,
            pos,
            force: Some(0.4),
        };
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let mut frames = vec![Vec::new(), Vec::new()];
        frames.push(vec![touch(egui::TouchPhase::Start, at), Event::PointerMoved(at), button(at, true)]);
        for k in 1..=4 {
            let wobble = at + vec2(0.4 * k as f32, -0.3 * k as f32);
            frames.push(vec![touch(egui::TouchPhase::Move, wobble), Event::PointerMoved(wobble)]);
        }
        let end = at + vec2(1.6, -1.2);
        frames.push(vec![touch(egui::TouchPhase::End, end), button(end, false), Event::PointerGone]);
        frames.push(Vec::new());
        let (mut time, mut asked, mut held) = (0.0, None, None);
        for events in frames {
            time += 1.0 / 60.0;
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 900.0))),
                time: Some(time),
                events,
                ..RawInput::default()
            };
            let mut output = ctx.clone().run_ui(raw, |ui| {
                asked = asked.take().or(plugin_panel(
                    ui.ctx(),
                    &mut look,
                    &Plugins::new(),
                    &panel,
                    &mut held,
                    &mut Vec::new(),
                    &Strokes::new(),
                ));
            });
            output.textures_delta.clear();
        }
        assert_eq!(asked, Some(PanelTouch::Touch(1, 0.0)), "the switch turned off");
    }

    /// What a press and lift at `at` asks of a ring of three colours and two brushes about window
    /// pixel `about` (none: the window's middle), the window 1200 by 900 at a pixel a point.
    fn ring_tapped(about: Option<(f32, f32)>, at: Pos2) -> Option<PanelTouch> {
        let ctx = egui::Context::default();
        crate::look::install_fonts(&ctx);
        let brush = |index| procraft_core::PanelBrush { set: 0, index, name: "B".to_owned() };
        let colours = vec![0xFF0000, 0x00FF00, 0x0000FF];
        let panel = PluginPanel {
            title: "Palette".to_owned(),
            rows: vec![
                PanelRow::Colours { label: "C".to_owned(), colours, chosen: Some(1) },
                PanelRow::Brushes { label: "B".to_owned(), brushes: vec![brush(0), brush(1)], chosen: None },
            ],
            key_press: false,
            ring: true,
            at: about.map(|(x, y)| procraft_core::PanelPoint { x, y }),
        };
        let press = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let (mut time, mut asked) = (0.0, None);
        for step in 0..6 {
            time += 1.0 / 60.0;
            let events = match step {
                2 => vec![Event::PointerMoved(at)],
                3 => vec![press(at, true)],
                4 => vec![press(at, false)],
                _ => Vec::new(),
            };
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 900.0))),
                time: Some(time),
                events,
                ..RawInput::default()
            };
            let mut output = ctx.clone().run_ui(raw, |ui| {
                asked = asked.take().or(plugin_ring(ui.ctx(), &panel, &Strokes::new()));
            });
            output.textures_delta.clear();
        }
        asked
    }

    #[test]
    fn a_ring_opens_where_the_pen_was_and_tells_each_colour_and_brush_tapped() {
        let pen = pos2(400.0, 300.0);
        let about = Some((400.0, 300.0));
        // the colours on the first ring from the top, clockwise; the brushes on the second
        assert_eq!(ring_tapped(about, pen + vec2(0.0, -RING_FIRST)), Some(PanelTouch::Touch(0, 0.0)));
        let second = ring_item(pen, 0, 1, 3, RING_COLOUR).0;
        assert_eq!(ring_tapped(about, second), Some(PanelTouch::Touch(0, 1.0)));
        let below = pen + vec2(0.0, RING_FIRST + RING_STEP);
        assert_eq!(ring_tapped(about, below), Some(PanelTouch::Touch(1, 1.0)));
        // between the rings nothing, off the backdrop closed
        assert_eq!(ring_tapped(about, pen + vec2(0.0, 30.0)), None);
        assert_eq!(ring_tapped(about, pos2(1000.0, 800.0)), Some(PanelTouch::Close));
        // from its card: about the window's middle
        let middle = pos2(600.0, 450.0);
        assert_eq!(ring_tapped(None, middle + vec2(0.0, -RING_FIRST)), Some(PanelTouch::Touch(0, 0.0)));
        // by a window's corner: moved in so it shows whole
        let reach = RING_FIRST + RING_STEP + RING_BRUSH / 2.0 + RING_MARGIN;
        let kept = pos2(reach, reach);
        assert_eq!(ring_tapped(Some((5.0, 5.0)), kept + vec2(0.0, -RING_FIRST)), Some(PanelTouch::Touch(0, 0.0)));
    }

    #[test]
    fn a_plugin_panel_bigger_or_smaller_takes_its_touches_where_they_show() {
        let row = |i: f32| move |r: Rect| pos2(r.left() + 60.0, r.top() + 60.0 + 22.0 + i * 45.0);
        for scale in [0.5, 1.5] {
            assert_eq!(panel_tapped_at(scale, row(1.0)), Some(PanelTouch::Touch(1, 0.0)), "{scale}");
            assert_eq!(panel_tapped_at(scale, row(3.0)), Some(PanelTouch::Touch(3, 1.0)), "{scale}");
            let done = |r: Rect| pos2(r.right() - 40.0, r.top() + 33.0);
            assert_eq!(panel_tapped_at(scale, done), Some(PanelTouch::Close), "{scale}");
        }
    }

    const SHADER: &str = "fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> { return colour; }";
    const SHADER_SHA: &str = "55f0107972b862c96fe458e60620daf4d655f0d1dfb7704a6358a20e6a4c5891";

    /// A plugin's module, as small as one with a slider can be: every sample goes to the corner.
    const MODULE: &str = "0061736d01000000010c0260027d7d017e60027f7d0003030200010728021170726f63726166745f706f736974696f6e00001070726f63726166745f636f6e74726f6c00010a0902040042000b02000b";
    const MODULE_SHA: &str = "1cc0369933a504bea35bd0c6b656f6bb97c5fe6b183a1c03dfc341d50be7c2d9";

    /// A plugin file with code in it, as small as one can be.
    fn code_file() -> Vec<u8> {
        code_file_with(r#"[{"key":"cut","type":"slider","label":"Cut","least":0,"most":32,"step":1,"starts":0}]"#)
    }

    /// A plugin file of code with `controls`.
    fn code_file_with(controls: &str) -> Vec<u8> {
        let module: Vec<u8> =
            (0..MODULE.len() / 2).map(|i| u8::from_str_radix(&MODULE[2 * i..2 * i + 2], 16).unwrap()).collect();
        let manifest = format!(
            r#"{{"schema":1,"id":"org.example.plugin","name":"A plugin","version":"1.0.0","holds":["hooks"],
            "publisher":{{"name":"Someone"}},"description":"Changes the app.","compat":{{"procraft":{{"interface":1}}}},
            "files":[{{"path":"plugin.wasm","sha256":"{MODULE_SHA}","size":{}}}],"permissions":[],"hooks":["position"],
            "controls":{controls}}}"#,
            module.len()
        );
        let mut zip = procraft_engine::zip::ZipWriter::new();
        zip.add("craft-extension.json", manifest.as_bytes());
        zip.add("plugin.wasm", &module);
        zip.finish()
    }

    /// A plugin file with a filter in it, as small as one can be.
    fn filter_file() -> Vec<u8> {
        let manifest = format!(
            r#"{{"schema":1,"id":"org.example.filter","name":"A filter","version":"1.0.0","holds":["filter"],
            "publisher":{{"name":"Someone"}},"description":"Changes a layer.","compat":{{"procraft":{{"interface":1}}}},
            "files":[{{"path":"filter.wgsl","sha256":"{SHADER_SHA}","size":{}}}],"permissions":[],"controls":[]}}"#,
            SHADER.len()
        );
        let mut zip = procraft_engine::zip::ZipWriter::new();
        zip.add("craft-extension.json", manifest.as_bytes());
        zip.add("filter.wgsl", SHADER.as_bytes());
        zip.finish()
    }

    fn body() -> Rect {
        Rect::from_min_size(pos2(100.0, 50.0), vec2(ACTIONS_WIDTH, 1400.0))
    }

    fn press(at: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    /// The Plugins page, with no window.
    struct Page {
        ctx: egui::Context,
        look: Look,
        actions: Actions,
        prefs: Prefs,
        time: f64,
    }

    impl Page {
        fn new() -> Page {
            let ctx = egui::Context::default();
            crate::look::install_fonts(&ctx);
            let (look, actions, prefs) = (Look::default(), Actions::default(), Prefs::default());
            let mut page = Page { ctx, look, actions, prefs, time: 0.0 };
            page.frame(Vec::new());
            page.frame(Vec::new());
            page
        }

        /// A frame, and what it asked of the app.
        fn asked(&mut self, events: Vec<Event>) -> Option<Act> {
            self.run(events).0
        }

        /// A frame: what it asked of the app, and what it drew.
        fn run(&mut self, events: Vec<Event>) -> (Option<Act>, Vec<egui::epaint::ClippedShape>) {
            self.time += 1.0 / 60.0;
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 1600.0))),
                time: Some(self.time),
                events,
                ..RawInput::default()
            };
            let Page { ctx, look, actions, prefs, .. } = self;
            let mut asked = None;
            let mut output = ctx.clone().run_ui(raw, |ui| {
                if let Some(Tapped::Act(act)) = actions.plugins_page(ui, look, body(), prefs) {
                    asked = Some(act);
                }
                if actions.plugin_menu(&ui.ctx().clone(), look, prefs, &mut Vec::new()) {
                    asked = Some(Act::Prefs);
                }
                if actions.remove_question(&ui.ctx().clone(), look, prefs) {
                    asked = Some(Act::Prefs);
                }
            });
            output.textures_delta.clear();
            (asked, output.shapes)
        }

        /// A frame: whether it said a value changed.
        fn frame(&mut self, events: Vec<Event>) -> bool {
            self.asked(events) == Some(Act::Prefs)
        }

        /// A press and a lift at `at`: what they asked of the app.
        fn tap_asks(&mut self, at: Pos2) -> Option<Act> {
            self.frame(vec![Event::PointerMoved(at)]);
            let pressed = self.asked(vec![press(at, true)]);
            let lifted = self.asked(vec![press(at, false)]);
            let after = self.asked(Vec::new());
            let settled = pressed.or(lifted).or(after);
            self.frame(Vec::new());
            settled
        }

        fn tap(&mut self, at: Pos2) -> bool {
            self.tap_asks(at) == Some(Act::Prefs)
        }

        fn list(&self) -> Vec<PluginInfo> {
            self.prefs.plugins.plugins()
        }

        /// Each card's top and bottom on the page.
        fn cards(&self) -> Vec<(f32, f32)> {
            let painter = self.ctx.layer_painter(egui::LayerId::background());
            let mut top = body().top() + 12.0 + 44.0 + 8.0;
            let mut cards = Vec::new();
            for plugin in self.list() {
                let bottom = top + plugin_height(&painter, &plugin, ACTIONS_WIDTH - 24.0);
                cards.push((top, bottom));
                top = bottom + GROUP_GAP;
            }
            cards
        }

        /// The middle of a card's row, `up` rows from its last.
        fn on_row(&self, card: usize, up: f32) -> Pos2 {
            pos2(body().center().x, self.cards()[card].1 - ROW_HEIGHT / 2.0 - up * (ROW_HEIGHT + 1.0))
        }

        /// The middle of the words `label` on the question Remove asks, once it has come in full.
        fn on_question(&mut self, label: &str) -> Pos2 {
            assert!(self.actions.removing.is_open(), "no question");
            std::thread::sleep(std::time::Duration::from_millis(350));
            let shapes = self.run(Vec::new()).1;
            // (its button is right of the card's row of the same words)
            drawn_all(&shapes, label).into_iter().max_by(|a, b| a.x.total_cmp(&b.x)).expect(label)
        }

        /// Card `card`'s Remove tapped, and the question answered Remove.
        fn remove(&mut self, card: usize) {
            let at = self.on_row(card, 0.0);
            assert!(!self.tap(at), "it asks first");
            let at = self.on_question("Remove");
            assert!(self.tap(at));
            assert!(!self.actions.removing.is_open());
        }

        /// A point on the first card's slider, `along` its way (0..1): over its two action rows.
        fn on_slider(&self, along: f32) -> Pos2 {
            let (left, right) = (body().left() + 12.0 + 16.0, body().right() - 12.0 - 16.0);
            let y = self.cards()[0].1 - 2.0 * (ROW_HEIGHT + 1.0) - SLIDE_HEIGHT / 2.0;
            pos2(left + along * (right - left), y)
        }

        /// The value of the first card's slider.
        fn slider_value(&self) -> f32 {
            match &self.list()[0].controls[0] {
                PluginControl::Slider { control } => control.value,
                other => panic!("{other:?}"),
            }
        }
    }

    /// The Plugins page with a plugin of one slider on, and where its files are.
    fn with_slider(name: &str) -> (Page, std::path::PathBuf) {
        let (mut page, dir) = with_store(name);
        assert_eq!(page.prefs.plugins.install(code_file()), None);
        assert!(page.prefs.plugins.set_on("org.example.plugin".to_owned(), true));
        page.frame(Vec::new());
        page.frame(Vec::new());
        (page, dir)
    }

    #[test]
    fn a_plugin_s_slider_goes_in_as_it_lets_go() {
        let (mut page, dir) = with_slider("slider");
        let (end, start) = (page.on_slider(1.0), page.on_slider(0.5));
        page.frame(vec![Event::PointerMoved(end)]);
        page.frame(vec![press(end, true)]);
        page.frame(Vec::new());
        // held, the row follows and nothing else does
        assert_eq!(page.actions.plugin_moved.as_ref().map(|m| m.1), Some(32.0));
        assert_eq!(page.slider_value(), 0.0);
        page.frame(vec![Event::PointerMoved(start)]);
        page.frame(Vec::new());
        assert_eq!(page.actions.plugin_moved.as_ref().map(|m| m.1), Some(16.0));
        assert_eq!(page.slider_value(), 0.0);
        let lifted = page.frame(vec![press(start, false)]);
        let after = page.frame(Vec::new());
        assert!(lifted || after);
        assert_eq!(page.slider_value(), 16.0);
        assert!(page.actions.plugin_moved.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tap_along_a_plugin_s_slider_sets_it() {
        let (mut page, dir) = with_slider("slider-tap");
        let at = page.on_slider(0.25);
        assert!(page.tap(at));
        assert_eq!(page.slider_value(), 8.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_nothing_imported_the_plugins_page_is_empty_and_offers_import() {
        let mut page = Page::new();
        assert!(page.cards().is_empty());
        let at = pos2(body().right() - 12.0 - 36.0, body().top() + 12.0 + 22.0);
        assert_eq!(page.tap_asks(at), Some(Act::ImportPlugin));
    }

    #[test]
    fn a_plugin_s_setting_named_at_every_step_is_picked_from_a_menu() {
        let (mut page, dir) = with_store("choice");
        let controls = r#"[{"key":"cut","type":"slider","label":"Cut","least":0,"most":2,"step":1,"starts":0,
            "names":["Mask","Whole","Cut"]}]"#;
        assert_eq!(page.prefs.plugins.install(code_file_with(controls)), None);
        assert!(page.prefs.plugins.set_on("org.example.plugin".to_owned(), true));
        page.frame(Vec::new());
        page.frame(Vec::new());
        // a row above Turn off and Remove, not a slider: a click opens the menu and sets nothing
        let row = page.on_row(0, 2.0);
        assert!(!page.tap(row));
        assert!(page.actions.plugin_menu.is_open());
        assert_eq!(page.slider_value(), 0.0);
        // grown in full (it opens over a third of a second)
        std::thread::sleep(std::time::Duration::from_millis(350));
        page.frame(Vec::new());
        // the menu's third row: it hangs 8 above the row, 54 in from its right end
        let top = row.y - ROW_HEIGHT / 2.0 - 8.0;
        let third = pos2(body().right() - 12.0 - 54.0 + 127.0, top + 8.0 + 40.0 * 2.5);
        assert!(page.tap(third));
        assert_eq!(page.slider_value(), 2.0);
        assert!(!page.actions.plugin_menu.is_open());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A page on a folder of its own, for a test's files.
    fn with_store(name: &str) -> (Page, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("procraft-actions-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut page = Page::new();
        page.prefs.plugins = Plugins::with_store(dir.to_string_lossy().into_owned());
        (page, dir)
    }

    #[test]
    fn an_imported_filter_starts_on_and_is_turned_off_and_removed_by_its_rows() {
        let (mut page, dir) = with_store("filter");
        assert_eq!(page.prefs.plugins.install(filter_file()), None);
        page.frame(Vec::new());
        page.frame(Vec::new());
        assert_eq!(page.cards().len(), 1);
        assert!(page.list()[0].on);
        assert_eq!(page.prefs.plugins.filters().len(), 1);
        let at = page.on_row(0, 1.0);
        assert!(page.tap(at));
        assert!(!page.list()[0].on);
        assert!(page.prefs.plugins.filters().is_empty(), "off, it isn't under Adjustments");
        page.remove(0);
        assert!(page.cards().is_empty());
        assert!(!dir.join("org.example.filter").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_imported_plugin_with_code_starts_off_and_is_turned_on_off_and_removed_by_its_rows() {
        let (mut page, dir) = with_store("plugin");
        assert_eq!(page.prefs.plugins.install(code_file()), None);
        page.frame(Vec::new());
        page.frame(Vec::new());
        assert_eq!(page.cards().len(), 1);
        assert!(!page.list()[0].on);
        let at = page.on_row(0, 1.0);
        assert!(page.tap(at));
        assert!(page.list()[0].on);
        // on, its slider shows over the same two rows
        let at = page.on_row(0, 1.0);
        assert!(page.tap(at));
        assert!(!page.list()[0].on);
        page.remove(0);
        assert!(page.cards().is_empty());
        assert!(!dir.join("org.example.plugin").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_the_app_comes_with_has_a_card_each_off_as_any_import_and_removed_stays_removed() {
        let (mut page, dir) = with_store("bundled");
        let store = dir.to_string_lossy().into_owned();
        page.prefs.plugins = Plugins::with_bundled(store.clone());
        page.frame(Vec::new());
        page.frame(Vec::new());
        let list = page.list();
        assert!(list.len() >= 8, "{list:?}");
        for plugin in &list {
            assert!(plugin.source == PluginSource::File && !plugin.on, "{plugin:?}");
            assert!(matches!(card_rows(plugin)[..], [CardRow::TurnOn, CardRow::Remove]), "{}", plugin.name);
        }
        // its first row turns it on, and off again
        let at = page.on_row(0, 1.0);
        assert!(page.tap(at));
        assert!(page.list()[0].on);
        let at = page.on_row(0, 1.0);
        assert!(page.tap(at));
        assert!(!page.list()[0].on);
        // its last removes it, and it isn't added again on the next start
        page.remove(0);
        let gone = |plugins: &Plugins| plugins.plugins().iter().all(|p| p.id != list[0].id);
        assert!(gone(&page.prefs.plugins));
        let next = Plugins::with_bundled(store);
        assert!(gone(&next));
        assert_eq!(next.plugins().len(), list.len() - 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The middle of each place `words` were drawn among a frame's `shapes`.
    fn drawn_all(shapes: &[egui::epaint::ClippedShape], words: &str) -> Vec<Pos2> {
        fn find(shape: &egui::Shape, words: &str, found: &mut Vec<Pos2>) {
            match shape {
                egui::Shape::Text(t) if t.galley.text() == words => found.push(t.pos + t.galley.size() / 2.0),
                egui::Shape::Vec(all) => all.iter().for_each(|s| find(s, words, found)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        shapes.iter().for_each(|c| find(&c.shape, words, &mut found));
        found
    }

    #[test]
    fn finger_pressure_s_card_shows_its_sliders_once_it_is_on() {
        const ID: &str = "io.github.artcraftandroid.brush-extras";
        let dir = std::env::temp_dir().join(format!("procraft-actions-finger-pressure-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut page = Page::new();
        page.prefs.plugins = Plugins::with_bundled(dir.to_string_lossy().into_owned());
        assert!(page.list().iter().any(|p| p.id == ID && !p.on), "it comes with the app, off");
        let shapes = page.run(Vec::new()).1;
        assert!(drawn(&shapes, "Brush extras").is_some());
        assert!(drawn(&shapes, "Finger pressure").is_none(), "off, its features don't show");
        assert!(page.prefs.plugins.set_on(ID.to_owned(), true));
        page.frame(Vec::new());
        // on: a heading for each feature, Finger pressure's sliders once it is on
        let shapes = page.run(Vec::new()).1;
        assert!(drawn(&shapes, "Finger pressure").is_some() && drawn(&shapes, "Touch position offset").is_some());
        assert!(drawn(&shapes, "Speed influence").is_none(), "its feature off, its sliders don't show");
        assert!(page.prefs.plugins.set_switch(format!("{ID}.finger_pressure"), true));
        page.frame(Vec::new());
        let shapes = page.run(Vec::new()).1;
        let rows = [
            ("Speed influence", "70%"),
            ("Start taper", "20 pt"),
            ("End taper", "40 pt"),
            ("Least pressure", "10%"),
            ("Most pressure", "100%"),
        ];
        let mut tops = Vec::new();
        for (label, value) in rows {
            let (Some(at), Some(shown)) = (drawn(&shapes, label), drawn(&shapes, value)) else {
                panic!("{label} {value}")
            };
            tops.push(at.y);
            assert!(shown.x > at.x, "{value} right of {label}");
        }
        assert!(tops.windows(2).all(|w| w[0] < w[1]), "in the manifest's order: {tops:?}");
        // a taper at its least reads Off
        assert!(page.prefs.plugins.set_slider(format!("{ID}.start"), 0.0).is_some());
        page.frame(Vec::new());
        assert!(drawn(&page.run(Vec::new()).1, "Off").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Finger pressure's card, on, drawn on a headless GPU as the window draws the interface, at 2
    /// pixels a point, into `PROCRAFT_DUMP/finger-pressure-card.png` (no window opens):
    /// `PROCRAFT_DUMP=dir cargo test -p procraft-desktop dump_finger_pressure_card -- --ignored`
    #[test]
    #[ignore]
    fn dump_finger_pressure_card() {
        use procraft_gpu::wgpu;
        const ID: &str = "io.github.artcraftandroid.brush-extras";
        let Ok(out) = std::env::var("PROCRAFT_DUMP") else { return };
        let Ok(gpu) = procraft_gpu::Gpu::headless() else { return eprintln!("skipped: no GPU") };
        let dir = std::env::temp_dir().join(format!("procraft-actions-finger-dump-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut page = Page::new();
        page.prefs.plugins = Plugins::with_bundled(dir.to_string_lossy().into_owned());
        // its card alone, on
        for other in page.list().into_iter().filter(|p| p.id != ID) {
            assert!(page.prefs.plugins.remove(other.id).is_some());
        }
        assert!(page.prefs.plugins.set_on(ID.to_owned(), true));
        assert!(page.prefs.plugins.set_switch(format!("{ID}.finger_pressure"), true));
        page.frame(Vec::new());
        page.frame(Vec::new());
        let (_, bottom) = page.cards()[0];
        // drawn afresh at 2 pixels a point, keeping every texture the frames make (its fonts')
        let scale = 2.0;
        page.ctx = egui::Context::default();
        crate::look::install_fonts(&page.ctx);
        page.ctx.set_pixels_per_point(scale);
        let mut textures = Vec::new();
        let mut shapes = Vec::new();
        for _ in 0..4 {
            page.time += 1.0 / 60.0;
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 1600.0))),
                time: Some(page.time),
                ..RawInput::default()
            };
            let Page { ctx, look, actions, prefs, .. } = &mut page;
            let mut output = ctx.clone().run_ui(raw, |ui| {
                let _ = actions.plugins_page(ui, look, body(), prefs);
            });
            textures.extend(std::mem::take(&mut output.textures_delta.set));
            shapes = output.shapes;
        }
        let ctx = page.ctx.clone();
        assert_eq!(ctx.pixels_per_point(), scale);
        let size = [(1200.0 * scale) as u32, (1600.0 * scale) as u32];
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("card"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut renderer = egui_wgpu::Renderer::new(&gpu.device, format, egui_wgpu::RendererOptions::default());
        for (id, deltas) in &textures {
            for delta in deltas {
                renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
            }
        }
        let jobs = ctx.tessellate(shapes, scale);
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: size, pixels_per_point: scale };
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("card") });
        let uploads = renderer.update_buffers(&gpu.device, &gpu.queue, &mut encoder, &jobs, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("card"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
        }
        gpu.queue.submit(uploads.into_iter().chain([encoder.finish()]));
        let pixels = procraft_gpu::read_texture(&gpu, &target, size[0], size[1]).unwrap();
        let picture = image::RgbaImage::from_raw(size[0], size[1], pixels).unwrap();
        // the page from its top to under the card
        let (x, y) = (((body().left() - 8.0) * scale) as u32, ((body().top() - 8.0) * scale) as u32);
        let (w, h) = (((ACTIONS_WIDTH + 16.0) * scale) as u32, ((bottom - body().top() + 24.0) * scale) as u32);
        let card = image::imageops::crop_imm(&picture, x, y, w, h).to_image();
        let path = std::path::Path::new(&out).join("finger-pressure-card.png");
        card.save(&path).unwrap();
        println!("{}", path.display());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_asks_first_with_the_plugin_s_name_and_cancel_keeps_it() {
        let (mut page, dir) = with_store("remove-asks");
        assert_eq!(page.prefs.plugins.install(filter_file()), None);
        page.frame(Vec::new());
        page.frame(Vec::new());
        let at = page.on_row(0, 0.0);
        assert!(!page.tap(at));
        assert_eq!(page.cards().len(), 1, "nothing goes before it is answered");
        assert!(page.actions.modal(), "a press elsewhere only answers it");
        let asked = page.actions.removing.asking().cloned().unwrap();
        assert_eq!(asked.title, "Remove this plugin?");
        assert!(asked.message.contains("\u{201c}A filter\u{201d}"), "{}", asked.message);
        assert_eq!(asked.action, "Remove");
        assert!(asked.destructive);
        // Cancel keeps it, and the next Remove asks again
        let cancel = page.on_question("Cancel");
        assert!(!page.tap(cancel));
        assert!(!page.actions.removing.is_open());
        assert_eq!(page.cards().len(), 1);
        assert!(dir.join("org.example.filter").exists());
        page.remove(0);
        assert!(page.cards().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_plugin_card_held_by_its_name_and_dragged_onto_another_takes_its_place_for_good() {
        let (mut page, dir) = with_store("reorder");
        assert_eq!(page.prefs.plugins.install(filter_file()), None);
        assert_eq!(page.prefs.plugins.install(code_file()), None);
        page.frame(Vec::new());
        page.frame(Vec::new());
        let ids = |page: &Page| page.list().into_iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(&page), ["org.example.filter", "org.example.plugin"]);
        // on a card's name and words
        let head = |page: &Page, card: usize| pos2(body().center().x, page.cards()[card].0 + 30.0);
        let (from, onto) = (head(&page, 1), head(&page, 0));
        // moved at once, it is no hold: nothing lifts, nothing moves
        page.frame(vec![Event::PointerMoved(from)]);
        page.frame(vec![press(from, true)]);
        for i in 1..=6 {
            page.frame(vec![Event::PointerMoved(from + (onto - from) * (i as f32 / 6.0))]);
        }
        page.frame(vec![press(onto, false)]);
        page.frame(Vec::new());
        assert_eq!(ids(&page), ["org.example.filter", "org.example.plugin"]);
        // held still, it lifts; dragged onto the first and let go there, it takes its place
        page.frame(vec![Event::PointerMoved(from)]);
        page.frame(vec![press(from, true)]);
        std::thread::sleep(crate::library::HOLD + std::time::Duration::from_millis(50));
        page.frame(Vec::new());
        assert!(page.actions.card_press.as_ref().is_some_and(|p| p.lifted && p.card == 1));
        for i in 1..=6 {
            page.frame(vec![Event::PointerMoved(from + (onto - from) * (i as f32 / 6.0))]);
        }
        page.frame(vec![press(onto, false)]);
        page.frame(Vec::new());
        assert!(page.actions.card_press.is_none());
        assert_eq!(ids(&page), ["org.example.plugin", "org.example.filter"]);
        // and so it is on the next start
        let next = Plugins::with_store(dir.to_string_lossy().into_owned());
        let again: Vec<String> = next.plugins().into_iter().map(|p| p.id).collect();
        assert_eq!(again, ["org.example.plugin", "org.example.filter"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Where `words` were drawn among a frame's `shapes` (their top left), if they were.
    fn drawn(shapes: &[egui::epaint::ClippedShape], words: &str) -> Option<Pos2> {
        fn find(shape: &egui::Shape, words: &str) -> Option<Pos2> {
            match shape {
                egui::Shape::Text(t) if t.galley.text() == words => Some(t.pos),
                egui::Shape::Vec(all) => all.iter().find_map(|s| find(s, words)),
                _ => None,
            }
        }
        shapes.iter().find_map(|c| find(&c.shape, words))
    }

    #[test]
    fn the_plugins_page_scrolls_with_a_pen_dragged_up_it_as_a_touch_or_as_the_mouse_pointer() {
        // how far up a pen dragged 200 points up the first card takes the second card's name, the
        // interface told it is on a touch screen first if `told` (egui drags a list only once it
        // has had a touch, from the frame after)
        let scrolled = |touches: bool, told: bool| {
            let (mut page, dir) = with_store(&format!("scroll-{touches}-{told}"));
            page.prefs.plugins = Plugins::with_bundled(dir.to_string_lossy().into_owned());
            page.frame(Vec::new());
            page.frame(Vec::new());
            assert!(page.cards().last().is_some_and(|c| c.1 > body().bottom()), "the cards fit the page");
            if told {
                let mut raw = RawInput::default();
                crate::input::PenTouchScreen::default().tell(&mut raw, true);
                page.frame(raw.events);
            }
            let name = page.list()[1].name.clone();
            let before = drawn(&page.run(Vec::new()).1, &name).unwrap();
            // on the first card's words, under its name
            let from = pos2(body().center().x, page.cards()[0].0 + 40.0);
            let mut last = Vec::new();
            for events in crate::input::pen_drag(from, vec2(0.0, -200.0), touches) {
                last = page.run(events).1;
            }
            let _ = std::fs::remove_dir_all(&dir);
            before.y - drawn(&last, &name).unwrap().y
        };
        // the system's pen, which has touched before: its touches drag the list
        let by_touch = scrolled(true, true);
        assert!(by_touch > 150.0, "{by_touch}");
        // a tablet's pen moving the mouse pointer: egui drags a list only on a touch screen
        assert_eq!(scrolled(false, false), 0.0, "the mouse pointer alone");
        let by_pointer = scrolled(false, true);
        assert!(by_pointer > 150.0, "{by_pointer}");
    }

    /// A card for a plugin the app has no code for: it shows, and a tap on it changes nothing.
    #[test]
    fn a_plugin_nothing_answers_to_draws_and_does_nothing() {
        let stranger = PluginInfo {
            features: Vec::new(),
            brush_controls: Vec::new(),
            actions: Vec::new(),
            kinds: Vec::new(),
            layer_kinds: Vec::new(),
            permissions: Vec::new(),
            id: "someone.stranger".to_owned(),
            name: "Stranger".to_owned(),
            description: "No code answers to it.".to_owned(),
            source: PluginSource::File,
            holds: vec![procraft_core::PluginPart::Hooks],
            version: "1.0.0".to_owned(),
            publisher: "Someone".to_owned(),
            experimental: false,
            on: true,
            on_key: "someone.stranger.on".to_owned(),
            problem: None,
            controls: vec![
                PluginControl::Switch {
                    control: PluginSwitch {
                        key: "someone.stranger.switch".to_owned(),
                        label: "On".to_owned(),
                        on: false,
                        starts: false,
                    },
                },
                PluginControl::Slider {
                    control: PluginSlider {
                        key: "someone.stranger.slider".to_owned(),
                        label: "Amount".to_owned(),
                        least: 0.0,
                        most: 10.0,
                        step: 0.5,
                        value: 2.0,
                        starts: 2.0,
                        shown_times: 1.0,
                        unit: " px".to_owned(),
                        at_least: None,
                        names: Vec::new(),
                        live: false,
                    },
                },
            ],
        };
        let (ctx, plugins) = (egui::Context::default(), Plugins::new());
        crate::look::install_fonts(&ctx);
        let before = plugins.plugins();
        let mut time = 0.0;
        let mut moved = None;
        let mut frame = |events: Vec<Event>, moved: &mut Option<(String, f32)>| {
            time += 1.0 / 60.0;
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
                time: Some(time),
                events,
                ..RawInput::default()
            };
            let mut output = ctx.clone().run_ui(raw, |ui| {
                assert!(!plugin_card(ui, &stranger, 0, &plugins, moved, &mut None, &mut None, &mut None, &mut None).0);
            });
            output.textures_delta.clear();
        };
        frame(Vec::new(), &mut moved);
        // its switch's row, then its slider's: the first has nothing to set, the second is held
        for at in [pos2(200.0, 92.0), pos2(200.0, 140.0)] {
            frame(vec![Event::PointerMoved(at)], &mut moved);
            frame(vec![press(at, true)], &mut moved);
            frame(vec![press(at, false)], &mut moved);
            frame(Vec::new(), &mut moved);
        }
        assert_eq!(moved.map(|m| m.0), Some("someone.stranger.slider".to_owned()));
        assert_eq!(plugins.plugins(), before);
    }
}
