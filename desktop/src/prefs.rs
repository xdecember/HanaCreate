//! The Prefs tab's settings, Pressure and Smoothing's and Advanced settings', kept between runs in
//! the app's folder (`interface`, a setting a line), as the Android app keeps them in its interface
//! preferences.

use std::sync::Arc;

use procraft_core::{CanvasSession, Plugins};

/// When the brush cursor shows: while the pointer hovers, while painting, or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShows {
    Hovering,
    Painting,
    Both,
}

/// How the brush cursor is drawn: light and dark rings, in the colour painted with, or as the
/// brush says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorStyle {
    Contrast,
    Colour,
    Brush,
}

/// Which floating panel's window is kept under `name`, by its place in
/// [`crate::floating_panels::PANELS`].
fn floating_at(name: &str) -> Option<usize> {
    crate::floating_panels::PANELS.iter().position(|&panel| procraft_core::floating_key(panel) == name)
}

/// What an artwork dragged out of the gallery becomes, and a picture's format when one is wanted.
pub const DRAG_FORMATS: [&str; 5] = ["HanaCreate", "PSD", "JPEG", "PNG", "TIFF"];
pub const DRAG_IMAGE_FORMATS: [&str; 3] = ["JPEG", "PNG", "TIFF"];

pub const RAPID_UNDO_MIN_S: f32 = 0.1;
pub const RAPID_UNDO_MAX_S: f32 = 1.5;

#[derive(Clone, Debug)]
pub struct Prefs {
    /// Panels and bars light grey with dark text.
    pub light_interface: bool,
    /// A selection drawn on a model reaches every surface under it.
    pub selection_x_ray: bool,
    /// The selection tool's Color Fill is on.
    pub selection_color_fill: bool,
    /// The side bar on the right edge instead of the left.
    pub right_handed: bool,
    /// The side bar's centre down its edge, a share of the window's height (its modify button
    /// dragged moves it).
    pub side_bar_centre: f32,
    /// Brush sizes relative to the screen (else to the canvas).
    pub brush_on_screen: bool,
    pub brush_cursor: bool,
    pub cursor_shows: CursorShows,
    pub cursor_style: CursorStyle,
    /// The colour model the Colors panel's disc and Value sliders work in.
    pub colour_model: procraft_core::ColourModel,
    /// How long two fingers are held before undo repeats, seconds.
    pub rapid_undo_delay: f32,
    /// How strongly what isn't selected is banded, 0..1.
    pub selection_visibility: f32,
    /// The app's own smoothing for every stroke, each 0..1, and its pressure curve.
    pub stabilization: f32,
    pub motion_filtering: f32,
    pub motion_expression: f32,
    pub pressure_smoothing: f32,
    pub tip_attached: bool,
    pub curve: Vec<[f32; 2]>,
    /// The Actions panel's tab open.
    pub actions_tab: usize,
    /// The Layers panel's and the brush library's windows as last left, while a plugin let them
    /// float (by their place in [`crate::floating_panels::PANELS`]).
    pub floating: [Option<procraft_core::FloatingWindow>; 2],
    /// The floating panels' places, as the core's layout keeps them (a place for each way the window
    /// is turned); none until one was kept. The windows above are where they start from.
    pub panel_layout: Option<String>,
    /// The interface's bars as Edit layout moved them, as the core's bar layout keeps them.
    pub bar_layout: Option<String>,
    /// Edit layout's named layouts, as the core keeps them (lines of their own).
    pub named_layouts: Option<String>,
    /// The brush studio's pad a sphere on every page.
    pub pad_3d: bool,
    /// The canvas shown on a second display too.
    pub project_canvas: bool,
    /// The Plugins page's list, its values kept here with the rest.
    pub plugins: Arc<Plugins>,
    /// Advanced settings: an artwork is kept turned as its canvas was left (the nearest quarter
    /// turn); fitting the canvas keeps it clear of the top bar and the side bar; new artworks don't
    /// record their time-lapse; strokes made in an adjustment undo together; how far the pen moves
    /// before it counts as fast (0.25..1); the colour named in a message as it changes.
    pub orientation_memory: bool,
    pub fit_inside: bool,
    pub no_time_lapse: bool,
    pub simplified_undos: bool,
    pub speed_distance: f32,
    pub colour_names: bool,
    /// A floating bar of undo, redo, zoom, move and fit for one finger; a tap landing where the
    /// finger first touched, even if it drifts before lifting.
    pub touch_companion: bool,
    pub tap_assistance: bool,
    /// Undo, redo, tools and panels click as they are used.
    pub feedback_sounds: bool,
    /// The short colour panel shows the history, not the active palette.
    pub colour_history_shown: bool,
    /// Drag and Drop export: what an artwork dragged out of the gallery becomes (an index into
    /// [`DRAG_FORMATS`]), and a picture's format when one is wanted (into [`DRAG_IMAGE_FORMATS`]).
    pub drag_format: usize,
    pub drag_image_format: usize,
    /// Recolor's Flood, kept from one time to the next.
    pub recolor_threshold: f32,
    /// Pen buttons: what the pen's first and second side buttons do, a QuickMenu action each, or
    /// (`None`, as they start) what Gesture controls say.
    pub pen_buttons: Vec<Option<procraft_core::QuickAction>>,
    /// Pen buttons: the plugins' actions (`<id>.<key>`) the pen's buttons run in place of
    /// `pen_buttons`, `None` for none; one runs only while its plugin is on.
    pub pen_button_plugins: Vec<Option<String>>,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            light_interface: false,
            selection_x_ray: false,
            selection_color_fill: false,
            right_handed: false,
            side_bar_centre: procraft_core::side_bar_first_centre(),
            brush_on_screen: false,
            brush_cursor: true,
            cursor_shows: CursorShows::Hovering,
            cursor_style: CursorStyle::Brush,
            colour_model: procraft_core::ColourModel::Hsb,
            rapid_undo_delay: 0.65,
            selection_visibility: 0.25,
            stabilization: 0.0,
            motion_filtering: 0.0,
            motion_expression: 0.0,
            pressure_smoothing: 0.0,
            tip_attached: true,
            curve: straight(),
            actions_tab: 0,
            floating: [None, None],
            panel_layout: None,
            bar_layout: None,
            named_layouts: None,
            pad_3d: false,
            project_canvas: false,
            plugins: Plugins::new(),
            orientation_memory: true,
            fit_inside: false,
            no_time_lapse: false,
            simplified_undos: true,
            speed_distance: 1.0,
            colour_names: false,
            touch_companion: false,
            tap_assistance: false,
            feedback_sounds: false,
            colour_history_shown: false,
            drag_format: 0,
            drag_image_format: 0,
            recolor_threshold: 0.0,
            pen_buttons: procraft_core::pen_button_actions(None),
            pen_button_plugins: procraft_core::pen_button_plugin_actions(None),
        }
    }
}

/// The pressure curve as it starts: straight.
pub fn straight() -> Vec<[f32; 2]> {
    vec![[0.0, 0.0], [1.0, 1.0]]
}

const SHOWS: [CursorShows; 3] = [CursorShows::Hovering, CursorShows::Painting, CursorShows::Both];
const STYLES: [CursorStyle; 3] = [CursorStyle::Contrast, CursorStyle::Colour, CursorStyle::Brush];

impl Prefs {
    /// As kept (anything missing or unreadable as it starts).
    pub fn kept() -> Prefs {
        let store = crate::files::app_dir().join("plugins").to_string_lossy().into_owned();
        let mut p = Prefs { plugins: Plugins::with_bundled(store), ..Prefs::default() };
        let text = std::fs::read_to_string(crate::files::app_dir().join("interface")).unwrap_or_default();
        for line in text.lines() {
            let Some((name, value)) = line.split_once(' ') else { continue };
            let on = value == "true";
            let number = value.parse::<f32>().ok().filter(|v| v.is_finite());
            let unit = number.map(|v| v.clamp(0.0, 1.0));
            match name {
                "light_interface" => p.light_interface = on,
                "selection_x_ray" => p.selection_x_ray = on,
                "selection_color_fill" => p.selection_color_fill = on,
                "right_handed" => p.right_handed = on,
                "brush_on_screen" => p.brush_on_screen = on,
                "brush_cursor" => p.brush_cursor = on,
                "cursor_shows" => p.cursor_shows = pick(&SHOWS, value).unwrap_or(p.cursor_shows),
                "cursor_style" => p.cursor_style = pick(&STYLES, value).unwrap_or(p.cursor_style),
                "colour_model" => {
                    p.colour_model = procraft_core::ColourModel::of_key(value).unwrap_or(p.colour_model);
                }
                "rapid_undo_delay" => {
                    if let Some(v) = number {
                        p.rapid_undo_delay = v.clamp(RAPID_UNDO_MIN_S, RAPID_UNDO_MAX_S);
                    }
                }
                "selection_visibility" => p.selection_visibility = unit.unwrap_or(p.selection_visibility),
                "stabilization" => p.stabilization = unit.unwrap_or(0.0),
                "motion_filtering" => p.motion_filtering = unit.unwrap_or(0.0),
                "motion_expression" => p.motion_expression = unit.unwrap_or(0.0),
                "pressure_smoothing" => p.pressure_smoothing = unit.unwrap_or(0.0),
                "tip_attached" => p.tip_attached = on,
                "pad_3d" => p.pad_3d = on,
                "project_canvas" => p.project_canvas = on,
                "orientation_memory" => p.orientation_memory = on,
                "fit_inside" => p.fit_inside = on,
                "no_time_lapse" => p.no_time_lapse = on,
                "simplified_undos" => p.simplified_undos = on,
                "speed_distance" => p.speed_distance = number.map_or(1.0, |v| v.clamp(0.25, 1.0)),
                "color_names" => p.colour_names = on,
                "single_touch_companion" => p.touch_companion = on,
                "tap_assistance" => p.tap_assistance = on,
                "feedback_sounds" => p.feedback_sounds = on,
                "color_history_shown" => p.colour_history_shown = on,
                "drag_format" => p.drag_format = value.parse::<usize>().map_or(0, |t| t.min(DRAG_FORMATS.len() - 1)),
                "drag_image_format" => {
                    p.drag_image_format = value.parse::<usize>().map_or(0, |t| t.min(DRAG_IMAGE_FORMATS.len() - 1))
                }
                "recolor_threshold" => p.recolor_threshold = unit.unwrap_or(0.0),
                "pen_buttons" => {
                    p.pen_buttons = procraft_core::pen_button_actions(Some(value.to_owned()));
                    p.pen_button_plugins = procraft_core::pen_button_plugin_actions(Some(value.to_owned()));
                }
                "actions_tab" => p.actions_tab = value.parse::<usize>().map_or(0, |t| t.min(5)),
                name if name == procraft_core::side_bar_centre_key() => {
                    p.side_bar_centre = procraft_core::side_bar_kept_centre(number.unwrap_or(f32::NAN));
                }
                name if name == procraft_core::panel_layout_key() => p.panel_layout = Some(value.to_owned()),
                name if name == procraft_core::bar_layout_key() => p.bar_layout = Some(value.to_owned()),
                // (its lines kept on one, a record separator between)
                name if name == procraft_core::named_layouts_key() => {
                    p.named_layouts = Some(value.replace('\u{1e}', "\n"));
                }
                name if floating_at(name).is_some() => {
                    if let Some(slot) = floating_at(name).and_then(|at| p.floating.get_mut(at)) {
                        *slot = procraft_core::floating_read(value.to_owned());
                    }
                }
                "curve" => {
                    let numbers: Vec<f32> = value.split(',').filter_map(|v| v.parse().ok()).collect();
                    let points: Vec<[f32; 2]> = numbers.as_chunks::<2>().0.to_vec();
                    if points.len() >= 2 {
                        p.curve = points;
                    }
                }
                _ => p.plugins.read_kept(name.to_owned(), value.to_owned()),
            }
        }
        // the interface shows as it was left
        crate::look::set_light(p.light_interface);
        p
    }

    pub fn keep(&self) {
        let curve: Vec<String> = self.curve.iter().map(|[x, y]| format!("{x},{y}")).collect();
        let lines = [
            format!("light_interface {}", self.light_interface),
            format!("selection_x_ray {}", self.selection_x_ray),
            format!("selection_color_fill {}", self.selection_color_fill),
            format!("right_handed {}", self.right_handed),
            format!("{} {}", procraft_core::side_bar_centre_key(), self.side_bar_centre),
            format!("brush_on_screen {}", self.brush_on_screen),
            format!("brush_cursor {}", self.brush_cursor),
            format!("cursor_shows {}", place(&SHOWS, self.cursor_shows)),
            format!("cursor_style {}", place(&STYLES, self.cursor_style)),
            format!("colour_model {}", self.colour_model.key()),
            format!("rapid_undo_delay {}", self.rapid_undo_delay),
            format!("selection_visibility {}", self.selection_visibility),
            format!("stabilization {}", self.stabilization),
            format!("motion_filtering {}", self.motion_filtering),
            format!("motion_expression {}", self.motion_expression),
            format!("pressure_smoothing {}", self.pressure_smoothing),
            format!("tip_attached {}", self.tip_attached),
            format!("curve {}", curve.join(",")),
            format!("actions_tab {}", self.actions_tab),
            format!("pad_3d {}", self.pad_3d),
            format!("project_canvas {}", self.project_canvas),
            format!("orientation_memory {}", self.orientation_memory),
            format!("fit_inside {}", self.fit_inside),
            format!("no_time_lapse {}", self.no_time_lapse),
            format!("simplified_undos {}", self.simplified_undos),
            format!("speed_distance {}", self.speed_distance),
            format!("color_names {}", self.colour_names),
            format!("single_touch_companion {}", self.touch_companion),
            format!("tap_assistance {}", self.tap_assistance),
            format!("feedback_sounds {}", self.feedback_sounds),
            format!("color_history_shown {}", self.colour_history_shown),
            format!("drag_format {}", self.drag_format),
            format!("drag_image_format {}", self.drag_image_format),
            format!("recolor_threshold {}", self.recolor_threshold),
            format!(
                "pen_buttons {}",
                procraft_core::pen_buttons_saved(self.pen_buttons.clone(), self.pen_button_plugins.clone())
            ),
        ];
        let floating = crate::floating_panels::PANELS.into_iter().zip(self.floating).filter_map(|(panel, window)| {
            Some(format!("{} {}", procraft_core::floating_key(panel), procraft_core::floating_kept(window?)))
        });
        let layout = self.panel_layout.iter().map(|kept| format!("{} {kept}", procraft_core::panel_layout_key()));
        let bars = self.bar_layout.iter().map(|kept| format!("{} {kept}", procraft_core::bar_layout_key()));
        let named = self
            .named_layouts
            .iter()
            .map(|kept| format!("{} {}", procraft_core::named_layouts_key(), kept.replace('\n', "\u{1e}")));
        let layout = layout.chain(bars).chain(named);
        let lines: Vec<String> =
            lines.into_iter().chain(floating).chain(layout).chain(self.plugins.kept_lines()).collect();
        crate::files::keep_text("interface", &(lines.join("\n") + "\n"));
    }

    /// The artwork follows them.
    pub fn apply(&self, session: &CanvasSession) {
        crate::look::set_light(self.light_interface);
        session.set_light_interface(self.light_interface);
        session.set_selection_x_ray(self.selection_x_ray);
        session.set_brush_scaling(self.brush_on_screen);
        session.set_selection_visibility(self.selection_visibility);
        session.use_plugins(Arc::clone(&self.plugins));
        session.set_simplified_undos(self.simplified_undos);
        session.set_speed_distance(self.speed_distance);
        // the turn an artwork is kept at follows its canvas's (Canvas Orientation Memory), or stays
        session.set_orientation_memory(self.orientation_memory);
        session.set_input_settings(
            self.stabilization,
            self.motion_filtering,
            self.motion_expression,
            self.pressure_smoothing,
            self.tip_attached,
            self.curve.iter().flatten().copied().collect(),
        );
    }

    /// The gesture settings as the canvas's touches take them, with the pen's buttons as Pen
    /// buttons set them.
    pub fn touch_settings(&self, gestures: &procraft_core::GestureSettings) -> procraft_core::TouchSettings {
        // a plugin's action while its plugin is on
        let on: Vec<String> =
            self.plugins.actions_in(procraft_core::PluginPlace::Pen).into_iter().map(|a| a.key).collect();
        let runs = |k: usize| self.pen_button_plugins.get(k).cloned().flatten().filter(|key| on.contains(key));
        procraft_core::TouchSettings {
            pen_button_does: self.pen_buttons.first().copied().flatten(),
            pen_second_button_does: self.pen_buttons.get(1).copied().flatten(),
            pen_button_runs: runs(0),
            pen_second_button_runs: runs(1),
            ..gestures.touch_settings(self.rapid_undo_ms())
        }
    }

    pub fn rapid_undo_ms(&self) -> f64 {
        f64::from(self.rapid_undo_delay) * 1000.0
    }

    /// Whether the cursor shows while the pointer hovers (else while painting).
    pub fn cursor_shown(&self, hovering: bool) -> bool {
        self.brush_cursor
            && if hovering {
                self.cursor_shows != CursorShows::Painting
            } else {
                self.cursor_shows != CursorShows::Hovering
            }
    }

    /// Pressure and Smoothing as they start.
    pub fn reset_input(&mut self) {
        self.stabilization = 0.0;
        self.motion_filtering = 0.0;
        self.motion_expression = 0.0;
        self.pressure_smoothing = 0.0;
        self.curve = straight();
    }
}

fn place<T: PartialEq>(list: &[T], v: T) -> usize {
    list.iter().position(|x| *x == v).unwrap_or(0)
}

fn pick<T: Copy>(list: &[T], value: &str) -> Option<T> {
    list.get(value.parse::<usize>().ok()?).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pen_button_runs_a_plugin_s_action_while_its_plugin_is_on_else_as_pen_buttons_say() {
        const PALETTE: &str = "io.github.artcraftandroid.pop-up-palette";
        let dir = std::env::temp_dir().join(format!("procraft-pen-plugins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let plugins = Plugins::with_bundled(dir.to_string_lossy().into_owned());
        let open = format!("{PALETTE}.open");
        let prefs = Prefs {
            plugins: Arc::clone(&plugins),
            pen_buttons: vec![Some(procraft_core::QuickAction::Undo), Some(procraft_core::QuickAction::Redo)],
            pen_button_plugins: vec![None, Some(open.clone())],
            ..Prefs::default()
        };
        let gestures = procraft_core::GestureSettings::new(procraft_core::KeptGestures::default());
        // it comes off: the button does what Pen buttons say besides
        let off = prefs.touch_settings(&gestures);
        assert_eq!((off.pen_button_runs, off.pen_second_button_runs), (None, None));
        assert_eq!(off.pen_second_button_does, Some(procraft_core::QuickAction::Redo));
        assert!(plugins.set_on(PALETTE.to_owned(), true));
        let on = prefs.touch_settings(&gestures);
        assert_eq!((on.pen_button_runs, on.pen_second_button_runs), (None, Some(open)));
        assert_eq!(on.pen_button_does, Some(procraft_core::QuickAction::Undo));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
