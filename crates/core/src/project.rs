//! Saving and opening artworks. An artwork is a folder:
//!
//! - `artwork.json`: the format, the title and the document (layers and their settings).
//! - `layers/<id>.png`: each layer's pixels, premultiplied RGBA8 stored losslessly in a PNG (an
//!   internal store, not for other apps).
//! - `layers/<id>.store`: what a brush keeps with the layer between strokes, if it keeps anything
//!   (see `layer_store.rs`).
//! - `thumbnail.png`: the picture at up to [`THUMBNAIL_SIDE`] pixels, for the gallery.
//!
//! Files are written beside their final name and renamed into place, so an interrupted save
//! leaves the previous version whole.

use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use procraft_engine::document::{BlendMode, Document, LayerId, LayerKind};
use procraft_engine::formats::{self, PsdKind, PsdLayer};

use crate::{CanvasSession, MobileError};

/// The gallery thumbnail's longest side, pixels.
pub const THUMBNAIL_SIDE: u32 = 512;
/// [`CanvasSession::edits_saved`] before any save.
pub(crate) const NOT_SAVED: u64 = u64::MAX;
/// How many saves leaving an artwork makes at most while it is painted on as it saves.
const LEAVE_SAVES: usize = 3;

/// What leaving an artwork came to (see [`CanvasSession::save_to_leave`]).
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum Leaving {
    /// Saved whole: the canvas may go.
    Left,
    /// It couldn't be saved: it stays open, with what to ask (a title and what it says): stay (the
    /// default), or leave without saving, which loses what wasn't saved.
    Stays { title: String, message: String },
}
/// How long after a change an open artwork kept saved looks for what to save (it waits for a
/// change first: nothing changing, it never looks).
const AUTOSAVE_EVERY: std::time::Duration = std::time::Duration::from_millis(if cfg!(test) { 50 } else { 3000 });
/// How soon it looks again when something under way kept it from saving: fast painting leaves
/// only short gaps between strokes, and what isn't saved is gone if the GPU's device goes.
const AUTOSAVE_AGAIN: std::time::Duration = std::time::Duration::from_millis(if cfg!(test) { 5 } else { 250 });
/// After a save, the autosave waits this many times as long as it took before it looks again (or
/// its usual time, if that is longer): each save reads the changed layers back from the GPU and
/// makes PNGs of them on the CPU, which on a slow device can take a second or more, so saving
/// takes at most a fifth of the time while painting goes on. Never longer apart than
/// [`AUTOSAVE_LONGEST`], so little is ever left unsaved.
const AUTOSAVE_SPACING: u32 = 4;
const AUTOSAVE_LONGEST: std::time::Duration = std::time::Duration::from_secs(30);

/// How long the autosave waits, looking `every` so long, after a save that took `took` (see
/// [`AUTOSAVE_SPACING`]).
fn autosave_wait(every: std::time::Duration, took: std::time::Duration) -> std::time::Duration {
    took.saturating_mul(AUTOSAVE_SPACING).min(AUTOSAVE_LONGEST).max(every)
}
/// A package's preview's longest side, pixels.
const PACKAGE_PREVIEW_SIDE: u32 = 1024;
const FORMAT: &str = "procraft-artwork";
const VERSION: u32 = 1;

/// Whether the artwork in `dir` plays: a model, which turns, or Animation Assist or Page Assist on, from its file alone.
#[uniffi::export]
pub fn artwork_plays(dir: String) -> bool {
    let text = fs::read_to_string(Path::new(&dir).join("artwork.json"));
    let file = text.ok().and_then(|t| serde_json::from_str::<ArtworkFile>(&t).ok());
    file.is_some_and(|f| f.document.animation.is_some() || f.model.is_some())
}

/// The longest side Preview's picture of an artwork is ever made at, pixels: past a screen's, it
/// would show nothing more.
pub const PREVIEW_SIDE_MOST: u32 = 4096;

/// The longest side to make Preview's picture of an artwork shown `width` × `height` (as the
/// gallery shows it, turned) at, for a `box_width` × `box_height` screen (pixels): as large as it
/// shows fitted there, so each of the screen's pixels has one of its own; never larger than the
/// artwork, nor than [`PREVIEW_SIDE_MOST`].
#[uniffi::export]
pub fn preview_side(width: u32, height: u32, box_width: u32, box_height: u32) -> u32 {
    let (w, h) = (f64::from(width.max(1)), f64::from(height.max(1)));
    let fit = (f64::from(box_width.max(1)) / w).min(f64::from(box_height.max(1)) / h);
    let side = (w.max(h) * fit).ceil().min(f64::from(u32::MAX)) as u32;
    side.clamp(1, width.max(height).clamp(1, PREVIEW_SIDE_MOST))
}

/// The artwork in `dir` as Preview shows it: the whole picture fitted to `max_side` pixels (see
/// [`preview_side`]), as its gallery thumbnail is made (its private layers left out, turned and in
/// its colours as the gallery shows it, its profile carried), as a PNG; none if it can't be opened
/// or drawn. Slow: the artwork is opened and drawn whole (on the GPU), then let go at once.
#[uniffi::export]
pub fn artwork_preview_png(dir: String, max_side: u32) -> Option<Vec<u8>> {
    let session = CanvasSession::open(dir).ok()?;
    let made = session.preview_png(max_side);
    session.release_canvas();
    made
}

impl CanvasSession {
    /// The picture [`artwork_preview_png`] makes, of this artwork opened for it.
    fn preview_png(&self, max_side: u32) -> Option<Vec<u8>> {
        // on the app's own device (made now if the app has none yet), not one of its own
        crate::device::warm_gpu();
        if !self.prepare_offscreen() {
            return None;
        }
        let (pending, icc, proof, turns) = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            let private = st.doc.private_layers();
            let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return None };
            canvas.render_finished(gpu, &mut st.doc);
            let pending = canvas.start_composite_thumbnail_without(gpu, &mut st.doc, max_side.max(1), &private);
            (pending, st.doc.picture_icc(), proof_table(&st.doc), st.turns)
        };
        let t = pending.pixels().ok()?;
        let (w, h, mut rgba) = turned(t.width, t.height, straight_alpha(&t.rgba), turns);
        if let Some(table) = proof {
            table.apply_rgba(&mut rgba, false);
        }
        png_bytes(w, h, &rgba, &icc).ok()
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ArtworkFile {
    format: String,
    version: u32,
    name: String,
    document: Document,
    /// Read as kept, or its default if it can't be (as are the document's own settings).
    #[serde(default, deserialize_with = "procraft_engine::document::or_default")]
    info: ArtworkInfo,
    /// How the gallery shows it: turned this many quarter turns clockwise (1 to 3), as its canvas
    /// was left; none, upright. Its thumbnail is kept turned so.
    #[serde(
        default,
        deserialize_with = "procraft_engine::document::or_default",
        skip_serializing_if = "Option::is_none"
    )]
    turns: Option<u8>,
    /// The model painted on, if it is one (its current texture set is `document`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<crate::scene3d::SavedModel>,
    /// The Reference window as it was left, if it was ever used (its picture beside the layers).
    #[serde(
        default,
        deserialize_with = "procraft_engine::document::or_default",
        skip_serializing_if = "Option::is_none"
    )]
    reference: Option<procraft_engine::reference_window::ReferenceWindow>,
    /// The Reference windows opened besides its own, as they were left (each picture beside the
    /// layers, `reference-2.png` and on).
    #[serde(
        default,
        deserialize_with = "procraft_engine::document::or_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    references: Vec<procraft_engine::reference_window::ReferenceWindow>,
    /// The kind of artwork it is, if a plugin makes it (`<id>.<key>`): the plugin's data is beside
    /// it in `plugin-data.bin`, and the layers are what it drew.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plugin: Option<String>,
    /// The layer a plugin's artwork's picture is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plugin_layer: Option<u64>,
    /// The values of plugins' controls kept with the artwork (`artwork` in their manifests), by
    /// their keys (`<id>.<key>`), a switch 1 or 0.
    #[serde(
        default,
        deserialize_with = "procraft_engine::document::or_default",
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    plugin_values: std::collections::BTreeMap<String, f32>,
}

/// A plugin's artwork's data in its folder.
pub(crate) const PLUGIN_DATA: &str = "plugin-data.bin";

/// An artwork's time-lapse in a package: whether it records, its video's settings (quality 0 to 3,
/// resolution 0 to 2, HEVC or H.264), and its video so far as MP4 files in order, `width` ×
/// `height` at `fps` and `bitrate` bits a second.
#[derive(Clone, Debug, uniffi::Record)]
pub struct PackageTimeLapse {
    pub recording: bool,
    pub quality: u8,
    pub resolution: u8,
    pub hevc: bool,
    pub segments: Vec<String>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u64,
}

/// What an artwork keeps about itself besides the picture.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct ArtworkInfo {
    /// When it was made, milliseconds since 1970 (0: not known).
    #[serde(default)]
    pub created_ms: u64,
    /// Strokes made on it.
    #[serde(default)]
    pub strokes: u64,
    /// Time spent on it, milliseconds.
    #[serde(default)]
    pub tracked_ms: u64,
    /// Who made it.
    #[serde(default)]
    pub author: String,
}

pub(crate) fn file_error(context: &str, e: impl std::fmt::Display) -> MobileError {
    MobileError::File { reason: format!("{context}: {e}") }
}

/// What a save writes, taken under the lock: the artwork's file, the layers not already in the
/// folder as they are (each with its hash) and the picture (if shown). What the GPU still copies
/// back is waited for after the lock is let go.
pub(crate) struct SaveSnapshot {
    /// The layers that had changed, counted as changed again if the save fails.
    changed: Vec<LayerId>,
    file: ArtworkFile,
    layers: Vec<(LayerId, u64, Vec<u8>)>,
    /// Layers on their way back from the GPU, to be checked against the folder once here.
    reading: Vec<(LayerId, procraft_gpu::Readback)>,
    /// Layers still the PNG files they were opened from (not shown yet), written as they are.
    files: Vec<(LayerId, Vec<u8>)>,
    /// The picture for the gallery, made small on the GPU (only it comes back, not the canvas).
    composite: Option<procraft_gpu::PendingThumbnail>,
    /// Not shown yet, with no thumbnail in the folder: one made from its top picture, for the
    /// gallery until the canvas draws its own.
    unshown: Option<(u32, u32, Vec<u8>)>,
    model: Option<crate::scene3d::ModelSnapshot>,
    /// The device read from, to tell whether it was lost meanwhile.
    gpu: Option<procraft_gpu::Gpu>,
    /// The Reference window's picture, if it has one.
    reference_picture: Option<Arc<Vec<u8>>>,
    /// The pictures of the windows opened besides it, each if it has one.
    more_pictures: Vec<Option<Arc<Vec<u8>>>>,
    /// A plugin's artwork's data, if it is one.
    plugin_data: Option<Vec<u8>>,
    /// The data of each plugin's layer (empty while it has none yet), for its file beside the
    /// layer's pixels; those whose data couldn't be read left out.
    layer_data: Vec<(LayerId, Arc<Vec<u8>>)>,
    /// The layers' stores (see `layer_store.rs`): those changed, their kind and their planes on
    /// their way back from the GPU; and those whose files there stay as they are.
    stores: Vec<(LayerId, u32, Vec<procraft_gpu::Readback>)>,
    stores_kept: Vec<LayerId>,
    /// Stores still the files they were opened from (not shown yet), written as they are.
    store_files: Vec<(LayerId, Vec<u8>)>,
}

#[cfg(test)]
impl SaveSnapshot {
    /// The layers the snapshot started back from the GPU (not taken as read ahead).
    pub(crate) fn read_again(&self) -> Vec<u64> {
        self.reading.iter().map(|(id, _)| id.0).collect()
    }
}

/// How much of a save's layers may still be on their way back from the GPU when the lock is let
/// go; past it they are read (and those unchanged let go) under it, or for the autosave read
/// ahead of its snapshot as much at a time (see [`LayersAhead`]).
const READ_AFTER_LOCK: usize = 256 << 20;

/// [`READ_AFTER_LOCK`], in tests [`TESTS_READ_AFTER_LOCK`].
fn read_after_lock() -> usize {
    if cfg!(test) { TESTS_READ_AFTER_LOCK.load(std::sync::atomic::Ordering::Relaxed) } else { READ_AFTER_LOCK }
}

/// The tests' [`READ_AFTER_LOCK`]: small, so their saves go every way (the app's for a benchmark).
pub(crate) static TESTS_READ_AFTER_LOCK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(64 << 10);

/// How long an autosave, once it has read ahead what changed, waits for a moment with nothing
/// under way (between strokes) to take its snapshot in, before it gives way and looks again later.
const SNAPSHOT_WAIT: std::time::Duration = std::time::Duration::from_secs(if cfg!(test) { 2 } else { 4 });
/// How often it looks for that moment.
const SNAPSHOT_LOOK: std::time::Duration = std::time::Duration::from_millis(8);
/// How many times a save asked for reads ahead what changed before the snapshot reads what is
/// still left in its hold (only when changes keep coming as it reads).
const SAVE_READS_AHEAD: u32 = 3;

/// Layers a save read ahead of its snapshot, outside the snapshot's hold of the canvas (see
/// [`CanvasSession::read_layers_ahead`]), painting going on meanwhile: each with its pixels'
/// version as read, and their hash with the pixels, or none where they are what the folder has.
/// The snapshot takes a layer as read when its version hasn't moved since (its pixels are the
/// same: every change gives a layer a new version, in the render that makes it), and reads again
/// only those whose has, so the save is the artwork at the snapshot's moment whatever was painted
/// while it read.
#[derive(Default)]
pub(crate) struct LayersAhead {
    layers: std::collections::HashMap<LayerId, ReadLayer>,
}

/// A layer read ahead (see [`LayersAhead`]).
struct ReadLayer {
    version: u64,
    hash: u64,
    /// None where the folder has these pixels already.
    rgba: Option<Vec<u8>>,
}

impl LayersAhead {
    /// Whether layer `id` was read at `version`.
    fn has(&self, id: LayerId, version: u64) -> bool {
        self.layers.get(&id).is_some_and(|l| l.version == version)
    }

    /// Layer `id` as read, if it was read at `version`: its hash and its pixels (none where the
    /// folder has them).
    fn take(&mut self, id: LayerId, version: u64) -> Option<(u64, Option<Vec<u8>>)> {
        if !self.has(id, version) {
            return None;
        }
        self.layers.remove(&id).map(|l| (l.hash, l.rgba))
    }

    /// How many layers are held read (for tests).
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.layers.len()
    }
}

/// What the last saves wrote: the folder, and a hash of each layer's pixels, so layers that
/// haven't changed aren't written again.
#[derive(Default)]
pub(crate) struct Saved {
    dir: PathBuf,
    layers: std::collections::HashMap<LayerId, u64>,
    sets: crate::scene3d::SavedSets,
    /// Which autosave may save: one started under another number has been stopped.
    autosave: u64,
    /// A hash of the Reference window's picture as the folder has it, if it has one, and of each
    /// picture of the windows opened besides it.
    reference_picture: Option<u64>,
    more_pictures: Vec<Option<u64>>,
    /// A hash of each plugin's layer's data as its file in the folder has it.
    pub(crate) layer_data: std::collections::HashMap<LayerId, u64>,
}

impl Saved {
    /// The hashes of the layers the last saves wrote to `dir`; none for another folder.
    pub(crate) fn known(&self, dir: &Path) -> std::collections::HashMap<LayerId, u64> {
        if self.dir == dir { self.layers.clone() } else { Default::default() }
    }

    /// An artwork opened from `dir`: its layer files are what the last save there wrote, their
    /// hashes known as each is read ([`Saved::read`]).
    pub(crate) fn opened(&mut self, dir: &Path) {
        self.dir = dir.to_path_buf();
        self.layers.clear();
        self.reference_picture = None;
        self.more_pictures.clear();
        self.layer_data.clear();
    }

    /// Layer `id` read from its file, its pixels' [`pixels_hash`]: a save that finds it unchanged
    /// leaves it.
    pub(crate) fn read(&mut self, id: LayerId, hash: u64) {
        self.layers.insert(id, hash);
    }
}

/// A quick hash of pixels, to tell a changed layer from one saved already.
pub(crate) fn pixels_hash(rgba: &[u8]) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01B3;
    let (words, rest) = rgba.as_chunks::<8>();
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for w in words {
        h = (h ^ u64::from_le_bytes(*w)).wrapping_mul(PRIME).rotate_left(29);
    }
    for &b in rest {
        h = (h ^ u64::from(b)).wrapping_mul(PRIME);
    }
    h
}

impl crate::State {
    /// The time counted since tracking last started goes into the artwork's (at most a day at
    /// once), and counts on from now.
    pub(crate) fn count_tracked_time(&mut self) {
        let Some(since) = self.tracking_since else { return };
        let now = std::time::Instant::now();
        let ms = u64::try_from(now.duration_since(since).as_millis()).unwrap_or(u64::MAX).min(24 * 3600 * 1000);
        self.info.tracked_ms = self.info.tracked_ms.saturating_add(ms);
        self.tracking_since = Some(now);
    }

    /// See [`CanvasSession::edits`]. A stroke counts from its lift, before the canvas lays it (and
    /// its step) at the next frame.
    pub(crate) fn edits(&self) -> u64 {
        let canvas = self.canvas.as_ref();
        let steps = canvas.map_or(0, |c| c.steps_made().wrapping_add(c.strokes_ended()));
        self.revision.wrapping_add(steps)
    }

    /// Nothing long under way that saving would end or cut short (a transform, an adjustment, text
    /// being edited, a selection or a fill being made): strokes aside, which come and go.
    pub(crate) fn settled(&self) -> bool {
        self.transform.is_none()
            && !self.adjusting
            && self.text_edit.is_none()
            && self.pending_doc.is_none()
            && self.feather_base.is_none()
            && self.selection_before.is_none()
            // a ColorDrop's fill still open
            && self.pour.is_none()
    }

    /// Nothing under way that saving would end or cut short: nothing long ([`State::settled`]) and
    /// no stroke.
    pub(crate) fn idle(&self) -> bool {
        self.settled()
            && self.stroke.is_none()
            && self.plugin_artwork.as_ref().is_none_or(|a| !a.down)
            && self.shape.is_none()
            && self.scene.as_ref().is_none_or(|s| !s.stroking)
    }

    /// The artwork as it is now, for saving into `dir`: a layer already there as it is (its hash
    /// among the `known` and its file there) is let go as soon as it is read, so a save holds only
    /// the layers that changed. Up to [`READ_AFTER_LOCK`] of the layers, and the picture, are
    /// only started back from the GPU here, to be waited for once the lock is let go; the rest
    /// are read here. Layers read `ahead` as they are now are taken as read. `bounded`: none is
    /// waited for here, the rest only started back; but if those come to more than
    /// [`READ_AFTER_LOCK`], nothing is taken and there is no snapshot (none: they are read ahead
    /// first).
    pub(crate) fn save_snapshot(
        &mut self,
        dir: &Path,
        known: &std::collections::HashMap<LayerId, u64>,
        mut ahead: Option<&mut LayersAhead>,
        bounded: bool,
    ) -> Result<Option<SaveSnapshot>, MobileError> {
        let changed = |id: LayerId, rgba: Vec<u8>| {
            let hash = pixels_hash(&rgba);
            let there = known.get(&id) == Some(&hash) && layer_path(dir, id).exists();
            (!there).then_some((id, hash, rgba))
        };
        // the time on it up to now saved with it
        self.count_tracked_time();
        let model = self.model_snapshot()?;
        // the gallery's turn, as the canvas is left
        self.turns = self.shown_turns();
        let file = ArtworkFile {
            format: FORMAT.to_owned(),
            version: VERSION,
            name: self.name.clone(),
            document: self.doc.clone(),
            info: self.info.clone(),
            turns: Some(self.turns).filter(|&t| t != 0),
            model: model.as_ref().map(|m| m.saved.clone()),
            reference: Some(self.reference_window.clone()).filter(|w| !w.is_unused()),
            references: self.more_references.iter().map(|m| m.window.clone()).collect(),
            // (opened with its plugin off, it stays the plugin's: its kind and data kept as they were)
            plugin: self
                .plugin_artwork
                .as_ref()
                .map(|a| a.kind.clone())
                .or_else(|| self.plugin_opened.as_ref().map(|o| o.kind.clone())),
            plugin_layer: self
                .plugin_artwork
                .as_ref()
                .map(|a| a.layer.0)
                .or_else(|| self.plugin_opened.as_ref().and_then(|o| o.layer)),
            plugin_values: self.plugin_values.clone(),
        };
        let mut layers = Vec::new();
        let mut reading = Vec::new();
        let mut files = Vec::new();
        let mut composite = None;
        let mut unshown = None;
        let mut changed_ids = Vec::new();
        let (mut stores, mut stores_kept, mut store_files) = (Vec::new(), Vec::new(), Vec::new());
        // shown but its canvas still being made (on another thread): made now, so the save has its
        // picture for the gallery
        if self.gpu.is_some() && self.canvas.is_none() {
            self.ensure_canvas();
        }
        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
            canvas.render(gpu, &mut self.doc, None);
            let (w, h) = (self.doc.width(), self.doc.height());
            // bounded: what is left to read must come back after the hold, a lot of it at most
            if bounded {
                let changed = canvas.changed();
                let read = |id: LayerId| ahead.as_deref().is_some_and(|a| a.has(id, canvas.layer_version(id)));
                let unread = self
                    .doc
                    .pixel_layer_ids()
                    .filter(|&id| {
                        let wanted = changed.contains(&id) || !known.contains_key(&id) || !layer_path(dir, id).exists();
                        let left_as_it_is = self.unreadable.contains(&id) && !changed.contains(&id);
                        wanted && !left_as_it_is && !read(id)
                    })
                    .count();
                if unread > 1 && unread.saturating_mul(layer_read_bytes(w, h)) > read_after_lock() {
                    return Ok(None);
                }
            }
            let changed_now = canvas.take_changed();
            changed_ids = changed_now.iter().copied().collect();
            let mut read_all = || -> Result<(), MobileError> {
                // the layers with pixels (a group has none of its own)
                for id in self.doc.pixel_layer_ids() {
                    // unchanged since it was read or saved, its file there: left as it is, unread
                    if !changed_now.contains(&id) && known.contains_key(&id) && layer_path(dir, id).exists() {
                        continue;
                    }
                    // its file couldn't be read (the layer shows empty): never written over until
                    // the layer is painted on, and then kept aside first
                    if self.unreadable.contains(&id) {
                        if !changed_now.contains(&id) {
                            continue;
                        }
                        let (from, to) = (layer_path(dir, id), unreadable_path(dir, id));
                        if from.exists() && !to.exists() {
                            fs::copy(&from, &to).map_err(|e| file_error("keeping a layer's unreadable file", e))?;
                        }
                        self.unreadable.remove(&id);
                    }
                    // read ahead as it is now: taken as read (its pixels' version hasn't moved)
                    if let Some((hash, rgba)) = ahead.as_deref_mut().and_then(|a| a.take(id, canvas.layer_version(id)))
                    {
                        layers.extend(rgba.map(|rgba| (id, hash, rgba)));
                        continue;
                    }
                    let after: usize = reading.iter().map(|(_, r): &(LayerId, procraft_gpu::Readback)| r.bytes()).sum();
                    let read = canvas.start_reading_layer(gpu, id).map_err(|e| file_error("reading a layer", e))?;
                    // (bounded, what is left was found to be little: all of it comes back after)
                    if after + read.bytes() <= read_after_lock() || bounded {
                        reading.push((id, read));
                    } else {
                        let rgba = read.pixels().map_err(|e| file_error("reading a layer", e))?;
                        layers.extend(changed(id, rgba));
                    }
                }
                Ok(())
            };
            // a layer that can't be read: those changed count as changed again, for the next save
            if let Err(e) = read_all() {
                canvas.mark_changed(changed_ids);
                return Err(e);
            }
            // the layers' stores: one whose layer is unchanged since a save into this folder stays
            // as its file is; a store that can't be read isn't saved (its paint dries as it shows)
            for (id, store) in canvas.stores() {
                if self.doc.layer(id).is_none() {
                    continue;
                }
                let path = crate::layer_store::store_path(dir, id);
                if !changed_now.contains(&id) && known.contains_key(&id) && path.exists() {
                    stores_kept.push(id);
                    continue;
                }
                let reads: Result<Vec<_>, _> =
                    store.planes.iter().map(|p| canvas.start_reading_layer(gpu, *p)).collect();
                match reads {
                    Ok(reads) => stores.push((id, store.kind, reads)),
                    Err(e) => log::error!("layer {id:?}'s store: {e}"),
                }
            }
            // for the gallery's thumbnail: private layers left out, the picture made small on the
            // GPU, so only that comes back (the whole canvas, read each save, was most of an
            // autosave's memory traffic on a large canvas)
            let private = self.doc.private_layers();
            canvas.render_finished(gpu, &mut self.doc);
            composite = Some(canvas.start_composite_thumbnail_without(gpu, &mut self.doc, THUMBNAIL_SIDE, &private));
        } else {
            // not shown yet: what was opened is still what there is. A layer still its file isn't
            // decoded: it is written as it is, or not at all if that file is there
            for (id, layer) in &self.pending_pixels {
                match layer {
                    Opened::File(from) => {
                        let to = layer_path(dir, *id);
                        if *from != to {
                            let bytes = fs::read(from).map_err(|e| file_error("reading a layer", e))?;
                            if fs::read(&to).ok().as_ref() != Some(&bytes) {
                                files.push((*id, bytes));
                            }
                        }
                    }
                    Opened::Png(bytes) => {
                        if fs::read(layer_path(dir, *id)).ok().as_ref() != Some(bytes) {
                            files.push((*id, bytes.clone()));
                        }
                    }
                    Opened::Pixels(rgba) => layers.extend(changed(*id, rgba.clone())),
                }
            }
            for (id, bytes) in &self.pending_stores {
                if fs::read(crate::layer_store::store_path(dir, *id)).ok().as_ref() != Some(bytes) {
                    store_files.push((*id, bytes.clone()));
                } else {
                    stores_kept.push(*id);
                }
            }
            // a picture made into an artwork and saved before it is shown (the app may be closed
            // before it is): the gallery shows the picture
            if !dir.join("thumbnail.png").exists() {
                let (w, h) = (self.doc.width(), self.doc.height());
                unshown = self.pending_pixels.iter().rev().find_map(|(_, layer)| match layer {
                    Opened::Pixels(rgba) => Some(thumbnail(rgba, w, h, THUMBNAIL_SIDE)),
                    Opened::File(_) | Opened::Png(_) => None,
                });
            }
        }
        // one whose file couldn't be read, deleted: its file kept aside before the save tidies it
        // away
        let gone: Vec<LayerId> = self.unreadable.iter().copied().filter(|id| self.doc.layer(*id).is_none()).collect();
        for id in gone {
            let (from, to) = (layer_path(dir, id), unreadable_path(dir, id));
            if from.exists() && !to.exists() {
                fs::copy(&from, &to).map_err(|e| file_error("keeping a layer's unreadable file", e))?;
            }
            self.unreadable.remove(&id);
        }
        let gpu = self.gpu.clone();
        let reference_picture = self.reference_picture.clone();
        let more_pictures = self.more_references.iter().map(|m| m.picture.clone()).collect();
        let plugin_data = self
            .plugin_artwork
            .as_ref()
            .map(|a| a.kept.clone())
            .or_else(|| self.plugin_opened.as_ref().and_then(|o| o.data.clone()));
        let layer_data = self
            .doc
            .layers()
            .iter()
            .filter(|l| !self.layer_data_unread.contains(&l.id))
            .filter_map(|l| Some((l.id, l.plugin.as_ref()?.data.clone().unwrap_or_default())))
            .collect();
        Ok(Some(SaveSnapshot {
            changed: changed_ids,
            file,
            layers,
            reading,
            files,
            composite,
            unshown,
            model,
            gpu,
            reference_picture,
            more_pictures,
            plugin_data,
            layer_data,
            stores,
            stores_kept,
            store_files,
        }))
    }
}

/// Writes a save; if it fails, the layers it had as changed count as changed again.
fn write_or_keep_changed(
    session: &CanvasSession,
    saved: &mut Saved,
    dir: &Path,
    snapshot: SaveSnapshot,
) -> Result<(), MobileError> {
    let changed = snapshot.changed.clone();
    let result = write_artwork(saved, dir, snapshot);
    if result.is_err()
        && let Some(canvas) = session.shared.lock().canvas.as_mut()
    {
        canvas.mark_changed(changed);
    }
    session.note_room(&result);
    result
}

impl CanvasSession {
    /// What a save of the artwork came to, for the storage (see `storage.rs`): failed for want of
    /// room, the storage is full; worked after a save or its time-lapse found it full, there is room
    /// again, and the next time it fills is told again.
    pub(crate) fn note_room(&self, result: &Result<(), MobileError>) {
        use std::sync::atomic::Ordering::Relaxed;
        match result {
            Err(e) if crate::storage::says_full(&e.to_string()) => {
                crate::storage::found_full();
                self.shared.found_full.store(true, Relaxed);
            }
            Ok(()) if self.shared.found_full.swap(false, Relaxed) => crate::storage::room_again(),
            _ => {}
        }
    }
}

impl CanvasSession {
    /// [`CanvasSession::start_autosave`], looking `every` so long, and `again` after that when
    /// something was under way.
    pub(crate) fn keep_saved(self: Arc<Self>, dir: String, every: std::time::Duration, again: std::time::Duration) {
        let kept = self.edits();
        self.keep_saved_since(dir, every, again, kept);
    }

    /// [`CanvasSession::keep_saved`], with what is in `dir` as of the artwork's edits `kept`: an
    /// artwork whose last save failed is saved at its next look even before it changes again.
    pub(crate) fn keep_saved_since(
        self: Arc<Self>,
        dir: String,
        every: std::time::Duration,
        again: std::time::Duration,
        mut kept: u64,
    ) {
        let mine = {
            let mut saved = self.shared.saving.lock().unwrap_or_else(|e| e.into_inner());
            saved.autosave = saved.autosave.wrapping_add(1);
            saved.autosave
        };
        let session = Arc::downgrade(&self);
        // (its own handle on the changes: waiting on them keeps nothing else of the session alive)
        let nudge = Arc::clone(&self.shared.nudge);
        let mut seen = nudge.seen();
        let started = std::thread::Builder::new().name("autosave".into()).spawn(move || {
            let mut wait = every;
            // how long it waits for a change before it looks again anyway, while something it
            // couldn't save yet waits (a tool left open): longer each time, none once all is saved
            let mut unsaved: Option<std::time::Duration> = None;
            loop {
                // nothing is looked for until something changes (or, the app away, it shows again)
                let Some(now) = nudge.wait_past(seen, unsaved) else { return };
                let woken = now != seen;
                seen = now;
                {
                    let Some(session) = session.upgrade() else { return };
                    if session.shared.saving.lock().unwrap_or_else(|e| e.into_inner()).autosave != mine {
                        return;
                    }
                    if session.shared.lock().hidden {
                        unsaved = None;
                        continue;
                    }
                }
                // changes come in runs (a stroke after another): saved a little after one, once
                // (a look again after a wait for nothing is made at once); the changes made
                // meanwhile are this look's, not a reason for another after it
                if woken {
                    std::thread::sleep(wait);
                    seen = nudge.seen();
                }
                wait = every;
                let Some(session) = session.upgrade() else { return };
                // (in the gap after a stroke: once the frames have laid its end, which reading the
                // layers would otherwise lay at once, touches and frames waiting on it)
                let _ = session.shared.await_frames_laid();
                let mut saved = session.shared.saving.lock().unwrap_or_else(|e| e.into_inner());
                if saved.autosave != mine {
                    return;
                }
                crate::activity::Counts::add(&session.shared.counts.autosave_looks);
                let now = session.edits();
                if now == kept {
                    unsaved = None;
                    continue;
                }
                let lost = || session.shared.lock().gpu.as_ref().is_some_and(procraft_gpu::Gpu::is_lost);
                if lost() {
                    return;
                }
                // (strokes, which come and go, the save waits for itself, reading ahead meanwhile)
                if !session.shared.lock().settled() {
                    wait = again;
                    unsaved = Some(unsaved.map_or(again, |w| (w * 2).min(AUTOSAVE_LONGEST)));
                    continue;
                }
                let started = std::time::Instant::now();
                let result = session.save_if_idle_with(&mut saved, &dir);
                // lost during the save: that is what failed it, and it tells of itself
                if result.is_err() && lost() {
                    return;
                }
                let mut failure = session.shared.save_failure.lock().unwrap_or_else(|e| e.into_inner());
                match result {
                    Ok(true) => {
                        kept = now;
                        failure.1 = false;
                        unsaved = None;
                        // a slow save, a longer wait before the next
                        wait = autosave_wait(every, started.elapsed());
                    }
                    Ok(false) => {
                        wait = again;
                        unsaved = Some(unsaved.map_or(again, |w| (w * 2).min(AUTOSAVE_LONGEST)));
                    }
                    Err(e) => {
                        log::error!("keeping the artwork saved: {e}");
                        if !failure.1 {
                            *failure = (Some(e.to_string()), true);
                        }
                        // tried again with the next change, or before long; a failing save (the
                        // storage full) reads and encodes as much as one that works, so it is
                        // spaced as one is
                        unsaved = Some(AUTOSAVE_LONGEST);
                        wait = autosave_wait(every, started.elapsed());
                    }
                }
            }
        });
        if let Err(e) = started {
            log::error!("keeping the artwork saved: {e}");
        }
    }

    /// [`CanvasSession::save_if_idle`] with what the last saves wrote (its lock held). The saving
    /// goes on beside the painting, the canvas held only a moment at a time: the layers that
    /// changed are read ahead (see [`LayersAhead`]) a lot at a time, each lot's copies started in
    /// a short hold and waited for once it is let go, strokes going on meanwhile; then, at a
    /// moment with nothing under way, the snapshot takes the document and those read as they
    /// still are, and starts back the few that changed since. Nothing is waited for under a hold,
    /// however many layers changed. False if no such moment came within [`SNAPSHOT_WAIT`] (looked
    /// for again later).
    fn save_if_idle_with(&self, saved: &mut Saved, dir: &str) -> Result<bool, MobileError> {
        let dir = Path::new(dir);
        let known = saved.known(dir);
        let mut ahead = LayersAhead::default();
        // read ahead first, and again only when the snapshot finds more left than it may start
        let mut read = true;
        let mut until = std::time::Instant::now();
        loop {
            // a model's layers are its colour's only while the snapshot is taken: read in its hold
            let flat = self.shared.lock().scene.is_none();
            if read {
                if flat {
                    self.read_layers_ahead(&mut ahead, dir, &known)?;
                }
                read = false;
                until = std::time::Instant::now() + SNAPSHOT_WAIT;
            }
            // a moment with nothing under way, and nothing the frames lay a share at a time (which
            // the snapshot's render would lay at once, the frames and touches waiting on it)
            if self.shared.quiet() {
                let mut guard = self.shared.lock_finished();
                let st = &mut *guard;
                if st.idle() {
                    // a model saves its colour as the artwork's document
                    let ahead = (flat && st.scene.is_none()).then_some(&mut ahead);
                    let bounded = ahead.is_some();
                    match st.with_colour(|st| st.save_snapshot(dir, &known, ahead, bounded))? {
                        Some(snapshot) => {
                            let edits = st.edits();
                            drop(guard);
                            write_or_keep_changed(self, saved, dir, snapshot)?;
                            self.shared.edits_saved.store(edits, std::sync::atomic::Ordering::Relaxed);
                            return Ok(true);
                        }
                        // more changed since the look ahead than may come back after the hold
                        None => read = true,
                    }
                }
            }
            if !read && std::time::Instant::now() >= until {
                return Ok(false);
            }
            std::thread::sleep(SNAPSHOT_LOOK);
        }
    }

    /// Reads ahead, outside the snapshot's hold (see [`LayersAhead`]), what a save into `dir` would
    /// read: the layers changed, or not in the folder as they are (`known` its layers' hashes),
    /// that weren't read already as they are now; as much at a time as may be on its way back
    /// from the GPU at once, the buffers made first, each lot's copies started in a short hold
    /// and waited for once it is let go. Painting may go on meanwhile: a layer it changes after
    /// it was read is read again by the snapshot (or by the next look ahead).
    pub(crate) fn read_layers_ahead(
        &self,
        ahead: &mut LayersAhead,
        dir: &Path,
        known: &std::collections::HashMap<LayerId, u64>,
    ) -> Result<(), MobileError> {
        // which layer files there are, looked at once outside the holds
        let there: std::collections::HashSet<LayerId> =
            known.keys().copied().filter(|&id| layer_path(dir, id).exists()).collect();
        // what to read, as the canvas is now
        let (gpu, bytes, ids) = {
            let st = self.shared.lock();
            let (Some(canvas), Some(gpu)) = (st.canvas.as_ref(), st.gpu.as_ref()) else { return Ok(()) };
            let changed = canvas.changed();
            let ids: Vec<LayerId> = st
                .doc
                .pixel_layer_ids()
                .filter(|&id| changed.contains(&id) || !there.contains(&id))
                // (one whose file couldn't be read is the snapshot's to see to)
                .filter(|id| !st.unreadable.contains(id))
                .filter(|&id| !ahead.has(id, canvas.layer_version(id)))
                .collect();
            (gpu.clone(), layer_read_bytes(st.doc.width(), st.doc.height()), ids)
        };
        let per_lot = (read_after_lock() / bytes.max(1)).max(1);
        for lot in ids.chunks(per_lot) {
            // the buffers the lot is copied into made before its hold
            gpu.keep_readbacks(bytes as u64, lot.len());
            let reads = {
                let st = self.shared.lock();
                let Some(canvas) = st.canvas.as_ref() else { return Ok(()) };
                let mut reads = Vec::new();
                for &id in lot {
                    let version = canvas.layer_version(id);
                    if st.doc.layer(id).is_none() || ahead.has(id, version) {
                        continue;
                    }
                    // (one the canvas hasn't made yet, added since its last render, is the snapshot's
                    // to read, after its render)
                    let Ok(read) = canvas.start_reading_layer(&gpu, id) else { continue };
                    reads.push((id, version, read));
                }
                reads
            };
            for (id, version, read) in reads {
                let rgba = read.pixels().map_err(|e| file_error("reading a layer", e))?;
                let hash = pixels_hash(&rgba);
                let as_there = known.get(&id) == Some(&hash) && there.contains(&id);
                ahead.layers.insert(id, ReadLayer { version, hash, rgba: (!as_there).then_some(rgba) });
            }
            #[cfg(test)]
            std::thread::sleep(std::time::Duration::from_millis(TESTS_READ_AHEAD_PAUSE.get()));
        }
        // a buffer for the snapshot's read of the layer painted on meanwhile, likewise
        gpu.keep_readbacks(bytes as u64, 1);
        Ok(())
    }
}

#[cfg(test)]
std::thread_local! {
    /// The tests' pause after each lot read ahead on this thread (ms), so painting goes on
    /// between lots.
    pub(crate) static TESTS_READ_AHEAD_PAUSE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The bytes a layer of a `width` x `height` canvas takes on its way back from the GPU (its rows
/// padded as a copy to a buffer has them).
fn layer_read_bytes(width: u32, height: u32) -> usize {
    (width as usize * 4).div_ceil(256) * 256 * height as usize
}

/// Writes a save into `dir` (made if missing): the layers that changed since the last save there,
/// the thumbnail and the document.
fn write_artwork(saved: &mut Saved, dir: &Path, snapshot: SaveSnapshot) -> Result<(), MobileError> {
    let SaveSnapshot {
        file,
        mut layers,
        reading,
        files,
        composite,
        unshown,
        model,
        gpu,
        reference_picture,
        more_pictures,
        plugin_data,
        layer_data,
        stores,
        stores_kept,
        store_files,
        ..
    } = snapshot;
    // what was still on its way back: those unchanged since the last save there are let go
    for (id, read) in reading {
        let rgba = read.pixels().map_err(|e| file_error("reading a layer", e))?;
        let hash = pixels_hash(&rgba);
        let there = saved.dir == dir && saved.layers.get(&id) == Some(&hash) && layer_path(dir, id).exists();
        if !there {
            layers.push((id, hash, rgba));
        }
    }
    let composite = composite
        .map(procraft_gpu::PendingThumbnail::pixels)
        .transpose()
        .map_err(|e| file_error("reading the picture", e))?;
    // a device lost meanwhile read back nothing true: the last save is kept as it is
    if gpu.as_ref().is_some_and(procraft_gpu::Gpu::is_lost) {
        return Err(file_error("reading the layers", "the graphics device was lost"));
    }
    if saved.dir != dir {
        saved.dir = dir.to_path_buf();
        saved.layers.clear();
        saved.sets.clear();
        saved.reference_picture = None;
        saved.more_pictures.clear();
        saved.layer_data.clear();
    }
    let (w, h) = (file.document.width(), file.document.height());
    fs::create_dir_all(dir.join("layers")).map_err(|e| file_error("making the folder", e))?;
    // Everything new is written beside its place first and only then put in place, the document
    // last: a save cut short while writing (the slow part) leaves the last one as it was. Written
    // in place one by one, a save after a crop cut short left layers of the new size beside a
    // document of the old, which the next open would drop.
    let mut staged: Vec<(PathBuf, PathBuf)> = Vec::new();
    let stage = |result: Result<PathBuf, MobileError>, to: PathBuf, staged: &mut Vec<(PathBuf, PathBuf)>| {
        result.map(|tmp| staged.push((tmp, to)))
    };
    // the layers whose stores are in the folder once this save is in place
    let mut stored = stores_kept;
    let written = (|| {
        // only those not already there as they are, encoded and written side by side, each
        // layer's PNG made in memory before it is written
        let each = w as usize * h as usize * 4;
        let results = procraft_engine::par::side_by_side_within(&layers, each, |(id, _, rgba)| {
            write_beside(&layer_path(dir, *id), |p| write_png(p, w, h, rgba, &[]))
        });
        // every one written kept track of, so a failure among them leaves none of them beside
        let mut failed = None;
        for ((id, _, _), result) in layers.iter().zip(results) {
            let result = result.unwrap_or_else(|| Err(file_error("writing a layer", "it stopped")));
            if let Err(e) = stage(result, layer_path(dir, *id), &mut staged) {
                failed.get_or_insert(e);
            }
        }
        if let Some(e) = failed {
            return Err(e);
        }
        for (id, bytes) in &files {
            let to = layer_path(dir, *id);
            stage(
                write_beside(&to, |p| crate::storage::write(p, bytes).map_err(|e| file_error("writing a layer", e))),
                to,
                &mut staged,
            )?;
        }
        if let Some(model) = model.as_ref() {
            crate::scene3d::write_model(&mut saved.sets, dir, model)?;
        }
        // a model's thumbnail is the model as last shown
        let thumb = dir.join("thumbnail.png");
        if let Some((pw, ph, rgba)) = model.and_then(|m| m.picture) {
            let (tw, th, small) = thumbnail(&rgba, pw, ph, THUMBNAIL_SIDE);
            stage(write_beside(&thumb, |p| write_png(p, tw, th, &small, &[])), thumb, &mut staged)?;
        } else if let Some((tw, th, small)) =
            composite.map(|t| (t.width, t.height, straight_alpha(&t.rgba))).or(unshown)
        {
            // turned as the gallery shows it
            let (tw, th, mut small) = turned(tw, th, small, file.turns.unwrap_or(0));
            // the gallery shows it in the artwork's colours (a CMYK canvas's: as its press prints them)
            if let Some(table) = proof_table(&file.document) {
                table.apply_rgba(&mut small, false);
            }
            let icc = file.document.picture_icc();
            stage(write_beside(&thumb, |p| write_png(p, tw, th, &small, &icc)), thumb, &mut staged)?;
        }
        // the Reference window's picture, unless the folder has it already
        let picture_path = dir.join(crate::reference_window::REFERENCE_PICTURE);
        if let Some(png) = reference_picture.as_deref()
            && (saved.reference_picture != Some(pixels_hash(png)) || !picture_path.exists())
        {
            let written = write_beside(&picture_path, |p| {
                crate::storage::write(p, png).map_err(|e| file_error("writing the reference picture", e))
            });
            stage(written, picture_path, &mut staged)?;
        }
        // those of the windows opened besides it likewise
        for (i, png) in more_pictures.iter().enumerate() {
            let path = dir.join(crate::reference_window::more_picture(i));
            let Some(png) = png.as_deref() else { continue };
            if saved.more_pictures.get(i).copied().flatten() == Some(pixels_hash(png)) && path.exists() {
                continue;
            }
            let written = write_beside(&path, |p| {
                crate::storage::write(p, png).map_err(|e| file_error("writing a reference picture", e))
            });
            stage(written, path, &mut staged)?;
        }
        // plugins' layers' data, each whole, unless its file there has it already
        for (id, data) in &layer_data {
            let path = layer_data_path(dir, *id);
            if saved.layer_data.get(id) == Some(&pixels_hash(data)) && path.exists() {
                continue;
            }
            let written = write_beside(&path, |p| {
                crate::storage::write(p, data.as_slice()).map_err(|e| file_error("writing a layer's plugin data", e))
            });
            stage(written, path, &mut staged)?;
        }
        // the layers' stores, each whole with the hash of its layer's pixels as saved now (one whose
        // layer's isn't known can't be told from a stale one when opened, so it isn't saved)
        for (id, kind, reads) in stores {
            let layer_hash =
                layers.iter().find(|(l, ..)| *l == id).map(|(_, h, _)| *h).or(saved.layers.get(&id).copied());
            let Some(layer_hash) = layer_hash else { continue };
            let planes = reads
                .into_iter()
                .map(|r| r.pixels().map_err(|e| file_error("reading a layer's store", e)))
                .collect::<Result<Vec<_>, _>>()?;
            let bytes = crate::layer_store::encode(kind, (w, h), layer_hash, &planes)?;
            let path = crate::layer_store::store_path(dir, id);
            let written = write_beside(&path, |p| {
                crate::storage::write(p, &bytes).map_err(|e| file_error("writing a layer's store", e))
            });
            stage(written, path, &mut staged)?;
            stored.push(id);
        }
        for (id, bytes) in &store_files {
            let path = crate::layer_store::store_path(dir, *id);
            let written = write_beside(&path, |p| {
                crate::storage::write(p, bytes).map_err(|e| file_error("writing a layer's store", e))
            });
            stage(written, path, &mut staged)?;
            stored.push(*id);
        }
        // a plugin's artwork's own data, whole, before the document that names its kind
        if let Some(data) = plugin_data.as_deref() {
            let path = dir.join(PLUGIN_DATA);
            let written = write_beside(&path, |p| {
                crate::storage::write(p, data).map_err(|e| file_error("writing the plugin's artwork", e))
            });
            stage(written, path, &mut staged)?;
        }
        let json = serde_json::to_string(&file).map_err(|e| file_error("writing the artwork", e))?;
        let doc = dir.join("artwork.json");
        stage(
            write_beside(&doc, |p| {
                crate::storage::write(p, json.as_bytes()).map_err(|e| file_error("writing the artwork", e))
            }),
            doc,
            &mut staged,
        )
    })();
    // in place, in the order written (the document last), or, short of that, none of it
    let placed = written.and_then(|()| {
        staged.iter().try_for_each(|(tmp, to)| fs::rename(tmp, to).map_err(|e| file_error("saving", e)))
    });
    if let Err(e) = placed {
        for (tmp, _) in &staged {
            let _ = fs::remove_file(tmp);
        }
        return Err(e);
    }
    for (id, hash, _) in &layers {
        saved.layers.insert(*id, *hash);
    }
    for (id, data) in &layer_data {
        saved.layer_data.insert(*id, pixels_hash(data));
    }
    // a picture taken away goes once the document without it is in place
    saved.reference_picture = reference_picture.as_deref().map(|png| pixels_hash(png));
    if reference_picture.is_none() {
        let _ = fs::remove_file(dir.join(crate::reference_window::REFERENCE_PICTURE));
    }
    saved.more_pictures = more_pictures.iter().map(|png| png.as_deref().map(|png| pixels_hash(png))).collect();
    for i in 0..crate::reference_window::MOST_REFERENCE_WINDOWS {
        if more_pictures.get(i).is_none_or(Option::is_none) {
            let _ = fs::remove_file(dir.join(crate::reference_window::more_picture(i)));
        }
    }
    for (id, _) in &files {
        // its pixels' hash isn't known: the next save writes it again
        saved.layers.remove(id);
    }
    // layer files the document no longer has (and any left beside by a save cut short), now that
    // the document saying so is in place
    saved.layers.retain(|id, _| file.document.layer(*id).is_some());
    saved.layer_data.retain(|id, _| file.document.layer(*id).is_some());
    if let Ok(entries) = fs::read_dir(dir.join("layers")) {
        // (a plugin's layer's data file stays while the layer is there, even one never read, under
        // its earlier name until there is one under its own)
        let plugins = file.document.layers().iter().filter(|l| l.plugin.is_some()).map(|l| {
            let path = layer_data_path(dir, l.id);
            if path.exists() { path } else { old_layer_data_path(dir, l.id) }
        });
        // (and a layer's store while it has one)
        let stores = stored.iter().map(|id| crate::layer_store::store_path(dir, *id));
        let keep: Vec<PathBuf> =
            file.document.layer_ids().map(|id| layer_path(dir, id)).chain(plugins).chain(stores).collect();
        for entry in entries.flatten() {
            let path = entry.path();
            if !keep.contains(&path) {
                let _ = fs::remove_file(path);
            }
        }
    }
    Ok(())
}

#[uniffi::export]
impl CanvasSession {
    /// A new artwork `width` × `height` at `dpi`, its background colour (sRGB 0..1, three values)
    /// and whether it shows, and its name: a custom canvas.
    #[uniffi::constructor]
    pub fn new_custom(
        width: u32,
        height: u32,
        dpi: f32,
        background: Vec<f32>,
        background_visible: bool,
        name: String,
    ) -> Arc<CanvasSession> {
        let most = procraft_engine::document::canvas_edge_most();
        let mut doc = Document::new(width.clamp(1, most), height.clamp(1, most));
        doc.dpi = dpi;
        if let [r, g, b] = background[..] {
            doc.background = [r, g, b];
        }
        doc.background_visible = background_visible;
        let name = if name.trim().is_empty() {
            "Untitled Artwork".to_owned()
        } else {
            name.trim().chars().take(256).collect()
        };
        CanvasSession::with_document(doc.repaired(), name, Vec::new())
    }

    /// A new artwork the size of `image` (straight RGBA8), the picture as its one layer; scaled
    /// down to fit if it is larger than a canvas may be here.
    #[uniffi::constructor]
    pub fn from_image(image: crate::layers::ThumbnailImage, name: String) -> Arc<CanvasSession> {
        let (sw, sh) = (image.width.max(1), image.height.max(1));
        let mut pixels = image.rgba;
        pixels.resize(sw as usize * sh as usize * 4, 0);
        for px in pixels.as_chunks_mut::<4>().0 {
            let a = u16::from(px[3]);
            for c in px.iter_mut().take(3) {
                *c = ((u16::from(*c) * a + 127) / 255) as u8;
            }
        }
        let most = procraft_engine::document::canvas_edge_most();
        let (w, h) = if sw.max(sh) > most {
            let k = most as f32 / sw.max(sh) as f32;
            let side = |v: u32| ((v as f32 * k).round() as u32).clamp(1, most);
            let (w, h) = (side(sw), side(sh));
            let to_old = procraft_engine::geom::Affine {
                a: sw as f32 / w as f32,
                d: sh as f32 / h as f32,
                ..procraft_engine::geom::Affine::IDENTITY
            };
            pixels = procraft_engine::crop::resample(
                &pixels,
                sw,
                sh,
                &procraft_engine::crop::Crop { width: w, height: h, to_old },
            );
            (w, h)
        } else {
            (sw, sh)
        };
        let doc = Document::new(w, h);
        let layer = doc.current();
        let name = if name.trim().is_empty() {
            "Untitled Artwork".to_owned()
        } else {
            name.trim().chars().take(256).collect()
        };
        CanvasSession::with_document(doc, name, vec![(layer, pixels)])
    }

    /// Opens an artwork saved with [`CanvasSession::save`].
    #[uniffi::constructor]
    pub fn open(dir: String) -> Result<Arc<CanvasSession>, MobileError> {
        let dir = PathBuf::from(dir);
        // noted as being opened before anything is read, until it shows (see `open_marks.rs`): an
        // opening that ends the app, even as its session is built, is asked about when next picked.
        // One that fails here is told, and its mark goes
        let open_mark = crate::open_marks::Mark::new(&dir);
        let text = fs::read_to_string(dir.join("artwork.json")).map_err(|e| file_error("reading the artwork", e))?;
        let file: ArtworkFile = serde_json::from_str(&text).map_err(|e| file_error("reading the artwork", e))?;
        if file.format != FORMAT || file.version > VERSION {
            return Err(MobileError::File {
                reason: format!("not an artwork this version can open ({} {})", file.format, file.version),
            });
        }
        let mut doc = file.document.repaired();
        // plugins' layers' data, as it is (an empty file: none yet). One whose file is gone or can't
        // be read takes no strokes, which would draw it again without what its pixels show, and
        // its file is left as it is
        let mut data_hashes = Vec::new();
        let mut data_unread = Vec::new();
        let plugin_layers: Vec<LayerId> = doc.layers().iter().filter(|l| l.plugin.is_some()).map(|l| l.id).collect();
        for id in plugin_layers {
            let path = layer_data_path(&dir, id);
            let path = if path.exists() { path } else { old_layer_data_path(&dir, id) };
            match fs::read(&path).or_else(|_| fs::read(&path)) {
                Ok(bytes) => {
                    data_hashes.push((id, pixels_hash(&bytes)));
                    doc.set_plugin_data(id, Some(Arc::new(bytes)).filter(|b| !b.is_empty()));
                }
                Err(e) => {
                    log::error!("layer {id:?}'s plugin data: {e}");
                    data_unread.push(id);
                }
            }
        }
        // each layer's file, read and decoded only as it goes to the GPU (an artwork of many
        // layers isn't all in memory as it opens), and its store's, if it has one
        let mut pngs = Vec::new();
        let mut stores = Vec::new();
        for layer in doc.layers() {
            if let Ok(bytes) = fs::read(crate::layer_store::store_path(&dir, layer.id)) {
                stores.push((layer.id, bytes));
            }
            let path = layer_path(&dir, layer.id);
            // a layer with no file is empty
            if path.exists() {
                pngs.push((layer.id, Opened::File(path)));
            }
        }
        let scene = file.model.as_ref().map(|m| crate::scene3d::open_saved_model(&dir, m)).transpose()?;
        let session = CanvasSession::with_document(doc, file.name, Vec::new());
        // its files are as the last save left them: one unchanged since isn't written again
        let reference_picture = fs::read(dir.join(crate::reference_window::REFERENCE_PICTURE)).ok();
        let more_references: Vec<crate::reference_window::MoreReference> = file
            .references
            .iter()
            .take(crate::reference_window::MOST_REFERENCE_WINDOWS - 1)
            .enumerate()
            .map(|(i, window)| crate::reference_window::MoreReference {
                window: window.clone(),
                picture: fs::read(dir.join(crate::reference_window::more_picture(i))).ok().map(Arc::new),
            })
            .collect();
        {
            let mut saving = session.shared.saving.lock().unwrap_or_else(|e| e.into_inner());
            saving.opened(&dir);
            saving.reference_picture = reference_picture.as_deref().map(pixels_hash);
            saving.more_pictures =
                more_references.iter().map(|m| m.picture.as_deref().map(|png| pixels_hash(png))).collect();
            saving.layer_data.extend(data_hashes);
        }
        {
            let mut st = session.shared.lock();
            st.open_mark = open_mark;
            st.pending_pixels = pngs;
            st.pending_stores = stores;
            st.layer_data_unread.extend(data_unread);
            st.info = file.info;
            st.turns = file.turns.unwrap_or(0) % 4;
            st.scene = scene.map(Box::new);
            st.reference_window = file.reference.unwrap_or_default();
            st.reference_picture = reference_picture.map(Arc::new);
            st.more_references = more_references;
            st.plugin_values = file.plugin_values.into_iter().filter(|(_, v)| v.is_finite()).collect();
            // a plugin's artwork: its kind and data, for the app to open it with its plugin
            st.plugin_opened = file.plugin.map(|kind| crate::plugin_artwork::PluginOpened {
                kind,
                data: fs::read(dir.join(PLUGIN_DATA)).ok(),
                layer: file.plugin_layer,
            });
        }
        Ok(session)
    }

    /// Saves the artwork into `dir` (made if missing): its document, every layer and a thumbnail.
    /// Saves started together run one after the other. An adjustment or transform still open (the
    /// app going into the background) stays open: its layers are saved as they show, and again
    /// once it ends.
    pub fn save(&self, dir: String) -> Result<(), MobileError> {
        // a time-lapse frame asked for and not taken yet (the frames stopped), and those on their
        // way to the disk, kept first: the app may make a video of them next
        self.keep_time_lapse();
        let mut saved = self.shared.saving.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Path::new(&dir);
        let known = saved.known(dir);
        // the layers that changed read ahead first, the canvas held only a moment at a time (as
        // the autosave reads them); after a few rounds, whatever is still left is read in the
        // snapshot's hold
        let mut ahead = LayersAhead::default();
        let mut round = 0;
        let (snapshot, edits) = loop {
            // (a model's layers are its colour's only while the snapshot is taken: read in its hold)
            let flat = self.shared.lock().scene.is_none();
            if flat {
                self.read_layers_ahead(&mut ahead, dir, &known)?;
            }
            let mut guard = self.shared.lock_finished();
            let st = &mut *guard;
            st.finish_stroke();
            let ahead = (flat && st.scene.is_none()).then_some(&mut ahead);
            let bounded = ahead.is_some() && round < SAVE_READS_AHEAD;
            // a model saves its colour as the artwork's document
            if let Some(snapshot) = st.with_colour(|st| st.save_snapshot(dir, &known, ahead, bounded))? {
                break (snapshot, st.edits());
            }
            round += 1;
        };
        write_or_keep_changed(self, &mut saved, dir, snapshot)?;
        self.shared.edits_saved.store(edits, std::sync::atomic::Ordering::Relaxed);
        // saves work again: the autosave's next failure is told
        self.shared.save_failure.lock().unwrap_or_else(|e| e.into_inner()).1 = false;
        Ok(())
    }

    /// Saves as [`CanvasSession::save`] does, but only when nothing is under way that saving
    /// would end (a stroke, a transform, an adjustment, text being edited...); false if it didn't.
    pub fn save_if_idle(&self, dir: String) -> Result<bool, MobileError> {
        let mut saved = self.shared.saving.lock().unwrap_or_else(|e| e.into_inner());
        self.save_if_idle_with(&mut saved, &dir)
    }

    /// A number that changes whenever the artwork does (new steps, undo and redo, layer
    /// settings): unchanged, there is nothing new to save.
    pub fn edits(&self) -> u64 {
        self.shared.lock().edits()
    }

    /// [`CanvasSession::edits`] as the last save that worked found them; none before one.
    pub fn edits_saved(&self) -> Option<u64> {
        Some(self.shared.edits_saved.load(std::sync::atomic::Ordering::Relaxed)).filter(|&e| e != NOT_SAVED)
    }

    /// Keeps the artwork saved into `dir` while it is open, from a thread of its own: a few
    /// seconds after it changes, as soon as nothing is under way that saving would cut short
    /// (only the layers that changed are written). The thread waits for a change: while nothing
    /// changes, or the app is away ([`CanvasSession::set_shown`]), it does nothing at all. Ends with [`CanvasSession::stop_autosave`],
    /// with another start, when the session is let go, or with the GPU's device: what that held
    /// can't be read, the last save stands, and [`CanvasSession::take_failure`] tells it.
    pub fn start_autosave(self: Arc<Self>, dir: String) {
        self.keep_saved(dir, AUTOSAVE_EVERY, AUTOSAVE_AGAIN);
    }

    /// Saves the artwork into `dir` to leave it: [`Leaving::Left`] once all of it is saved (what is
    /// painted while it saves saved too), and the canvas may go. An artwork is never left with work
    /// that is only in memory: one that can't be saved (the storage full, say) [`Leaving::Stays`]
    /// open, painting from memory and kept saved again (what wasn't saved goes at the autosave's next
    /// look, once there is room), with what to ask: stay, or leave without saving, which loses it.
    /// The apps all leave so; only the system ending the app can lose it.
    pub fn save_to_leave(self: Arc<Self>, dir: String) -> Leaving {
        self.stop_autosave();
        let mut failed = None;
        for _ in 0..LEAVE_SAVES {
            match self.save(dir.clone()) {
                Ok(()) if self.edits_saved() == Some(self.edits()) => return Leaving::Left,
                Ok(()) => failed = None,
                Err(e) => {
                    failed = Some(e.to_string());
                    break;
                }
            }
        }
        // (painted on through every round: what the last saved stands, the rest is the next save's)
        let Some(reason) = failed else { return Leaving::Left };
        let saved = self.shared.edits_saved.load(std::sync::atomic::Ordering::Relaxed);
        let ever_saved = Path::new(&dir).join("artwork.json").is_file();
        let (title, message) = crate::storage::leave_unsaved_said(&reason, ever_saved);
        Arc::clone(&self).keep_saved_since(dir, AUTOSAVE_EVERY, AUTOSAVE_AGAIN, saved);
        self.shared.nudge.nudge();
        Leaving::Stays { title, message }
    }

    /// Ends the autosave started last: a save it is making is finished first, and none follows.
    /// Why the autosave failed, once until a save works and another fails (for the app to tell,
    /// as [`save_failure_said`] puts it); none if it hasn't. Never waits on a save.
    pub fn take_save_failure(&self) -> Option<String> {
        self.shared.save_failure.lock().unwrap_or_else(|e| e.into_inner()).0.take()
    }

    pub fn stop_autosave(&self) {
        let mut saved = self.shared.saving.lock().unwrap_or_else(|e| e.into_inner());
        saved.autosave = saved.autosave.wrapping_add(1);
        // its thread, waiting for a change, sees it is to end
        self.shared.nudge.nudge();
    }

    /// Gives an artwork opened only to be exported (never shown) a GPU device, so its exports
    /// work: the app's own where it has made one (the backend chosen for this machine, not
    /// whichever comes first, as a driver that takes the app down on another may), else one of its
    /// own; false if there is none.
    pub fn prepare_offscreen(&self) -> bool {
        let app = crate::device::kept().clone();
        let mut st = self.shared.lock();
        if st.gpu.is_none() {
            match app.map_or_else(procraft_gpu::Gpu::headless, Ok) {
                Ok(gpu) => st.gpu = Some(gpu),
                Err(e) => {
                    log::error!("exporting offscreen: {e}");
                    return false;
                }
            }
        }
        st.ensure_canvas();
        st.canvas.is_some()
    }

    /// How far the export under way has come, 0 to 1 (read while it runs, without waiting on it):
    /// the layers read back take it to 0.7, the file made to 0.95, written to 1.
    pub fn export_progress(&self) -> f32 {
        f32::from_bits(self.shared.export_progress.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Starts the count again before an export (those that don't tell their progress stay at 0).
    pub fn reset_export_progress(&self) {
        self.exported(0.0);
    }

    pub(crate) fn exported(&self, done: f32) {
        self.shared.export_progress.store(done.clamp(0.0, 1.0).to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    /// Writes the flattened picture, full size and with straight alpha, to `path` as a PNG.
    pub fn export_png(&self, path: String) -> Result<(), MobileError> {
        let (w, h, straight, icc) = self.finished_picture()?;
        self.exported(0.7);
        write_atomically(Path::new(&path), |p| write_png(p, w, h, &straight, &icc))?;
        self.exported(1.0);
        Ok(())
    }

    /// Writes the flattened picture, full size and onto white, to `path` as a JPEG, with the
    /// canvas's colour profile.
    pub fn export_jpeg(&self, path: String) -> Result<(), MobileError> {
        let (w, h, straight, icc) = self.finished_picture()?;
        self.exported(0.7);
        let rgb = onto_white(&straight)?;
        drop(straight);
        let bytes = procraft_engine::jpeg::encode_tagged(&rgb, w, h, JPEG_QUALITY, &icc);
        self.exported(0.95);
        write_atomically(Path::new(&path), |p| crate::storage::write(p, &bytes).map_err(|e| file_error("saving", e)))?;
        self.exported(1.0);
        Ok(())
    }

    /// Opens a layered artwork package (as the Share menu's first row writes) from `path`.
    #[uniffi::constructor]
    pub fn open_package(path: String) -> Result<Arc<CanvasSession>, MobileError> {
        let bytes = fs::read(&path).map_err(|e| file_error("opening the file", e))?;
        let imported =
            procraft_engine::native::read(&bytes).map_err(|e| file_error("opening the file", format!("{e:?}")))?;
        let session = CanvasSession::with_document(imported.document, imported.name, imported.pixels);
        {
            let mut st = session.shared.lock();
            st.info.strokes = imported.strokes;
            st.info.tracked_ms = (imported.tracked_seconds * 1000.0) as u64;
            st.info.author = imported.author;
            st.reference_window = imported.reference;
            // upright, and no bigger than a window keeps a picture
            st.reference_picture = imported.reference_picture.map(|(png, turn)| {
                Arc::new(crate::reference_window::kept_picture(crate::reference_window::upright(png, turn)))
            });
        }
        Ok(session)
    }

    /// The artwork as a layered package at `path`, with its time-lapse; `seed` makes its layers' ids.
    pub fn export_package(
        &self,
        path: String,
        seed: u64,
        time_lapse: Option<PackageTimeLapse>,
    ) -> Result<(), MobileError> {
        let time_lapse = match time_lapse {
            Some(t) => {
                let mut segments = Vec::with_capacity(t.segments.len());
                for s in &t.segments {
                    segments.push(fs::read(s).map_err(|e| file_error("reading the time-lapse", e))?);
                }
                Some(procraft_engine::native::TimeLapse {
                    recording: t.recording,
                    quality: t.quality,
                    resolution: t.resolution,
                    hevc: t.hevc,
                    segments,
                    frame_size: (t.width, t.height),
                    fps: t.fps,
                    bitrate: t.bitrate,
                })
            }
            None => None,
        };
        self.exported(0.0);
        let (doc, composite, name, info, reference, reference_picture, turns, edits) = {
            let mut guard = self.shared.lock_finished();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else {
                return Err(file_error("reading the picture", "the canvas is not shown yet"));
            };
            // the file's preview leaves private layers out
            let private = st.doc.private_layers();
            let composite = canvas
                .read_composite_without(gpu, &mut st.doc, &private)
                .map_err(|e| file_error("reading the picture", e))?;
            let reference = (st.reference_window.clone(), st.reference_picture.clone());
            st.count_tracked_time();
            let turns = st.shown_turns();
            let edits = st.edits();
            (st.doc.clone(), composite, st.name.clone(), st.info.clone(), reference.0, reference.1, turns, edits)
        };
        let (w, h) = (doc.width(), doc.height());
        // the preview turned as the package says the canvas shows
        let (tw, th, thumb) = {
            let mut straight = composite.clone();
            straighten(&mut straight);
            thumbnail(&straight, w, h, PACKAGE_PREVIEW_SIDE)
        };
        let (tw, th, mut thumb) = turned(tw, th, thumb, turns);
        if let Some(table) = proof_table(&doc) {
            table.apply_rgba(&mut thumb, false);
        }
        let preview = png_bytes(tw, th, &thumb, &doc.picture_icc())?;
        // every layer and mask read as it is written, a layer at a time
        let count = doc.layers().iter().filter(|l| l.kind != LayerKind::Group).count();
        let pixels = ExportLayers::new(self, edits, count, (0.1, 0.9));
        let export = procraft_engine::native::Export {
            document: &doc,
            pixels: &pixels,
            composite: &composite,
            thumbnail_png: &preview,
            name: &name,
            author: &info.author,
            strokes: info.strokes,
            tracked_seconds: info.tracked_ms as f64 / 1000.0,
            seed,
            time_lapse: time_lapse.as_ref(),
            reference: Some(&reference),
            reference_picture: reference_picture.as_deref().map(Vec::as_slice),
            turns: u32::from(turns),
        };
        write_file_atomically(Path::new(&path), |out| procraft_engine::native::write_to(&export, out).map(|_| ()))?;
        self.exported(1.0);
        Ok(())
    }

    /// The artwork as a layered PSD (groups, masks, clipping, blend modes kept) at `path`, its
    /// layers read and written one at a time.
    pub fn export_psd(&self, path: String) -> Result<(), MobileError> {
        let (p, edits) = self.picture()?;
        let (w, h) = (p.width, p.height);
        let (layers, from) = psd_layers(&p.doc);
        let (icc, inks) = file_colours(&p.doc);
        let count = from.iter().map(|f| f.reads()).sum();
        let reader = ExportLayers::new(self, edits, count, (0.1, 0.9));
        let [r, g, b] = p.doc.background.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        let mut read = |i: usize| -> std::io::Result<formats::PsdPixels> {
            let pixels = match from.get(i) {
                Some(PsdFrom::Layer(id, mask)) => {
                    formats::PsdPixels { rgba: reader.straight(*id)?, mask: mask.map(|m| reader.grey(m)).transpose()? }
                }
                Some(PsdFrom::Background) => {
                    formats::PsdPixels { rgba: [r, g, b, 255].repeat(w as usize * h as usize), mask: None }
                }
                _ => return Err(std::io::Error::other("no such layer")),
            };
            Ok(pixels)
        };
        write_file_atomically(Path::new(&path), |out| {
            formats::write_psd(out, w, h, p.doc.dpi, &icc, &layers, &mut read, &p.composite, inks.as_ref())
        })?;
        self.exported(1.0);
        Ok(())
    }

    /// The flattened picture as a TIFF at `path`.
    pub fn export_tiff(&self, path: String) -> Result<(), MobileError> {
        let (p, _) = self.picture()?;
        let bytes = match file_colours(&p.doc) {
            (icc, Some(inks)) => formats::tiff_cmyk(p.width, p.height, p.doc.dpi, &icc, &p.composite, &inks),
            (icc, None) => formats::tiff(p.width, p.height, p.doc.dpi, &icc, &p.composite),
        };
        self.exported(0.95);
        write_atomically(Path::new(&path), |f| crate::storage::write(f, &bytes).map_err(|e| file_error("saving", e)))?;
        self.exported(1.0);
        Ok(())
    }

    /// The flattened picture, or each layer (`layers`), as a page of a PDF at `path`: exact at
    /// `quality` 100, JPEG below it. Each layer is read and written in turn.
    pub fn export_pdf(&self, path: String, layers: bool, quality: u8) -> Result<(), MobileError> {
        let (p, edits) = self.picture()?;
        let (icc, inks) = file_colours(&p.doc);
        let (w, h, quality) = (p.width, p.height, quality.min(100));
        if !layers {
            let bytes = formats::pdf(w, h, p.doc.dpi, &icc, &[p.composite.as_slice()], quality, inks.as_ref());
            self.exported(0.95);
            write_atomically(Path::new(&path), |f| {
                crate::storage::write(f, &bytes).map_err(|e| file_error("saving", e))
            })?;
            self.exported(1.0);
            return Ok(());
        }
        drop(p.composite);
        // a page for each paint layer, bottom first
        let pages: Vec<LayerId> = p.doc.layers().iter().filter(|l| l.kind == LayerKind::Paint).map(|l| l.id).collect();
        let reader = ExportLayers::new(self, edits, pages.len(), (0.1, 0.95));
        write_file_atomically(Path::new(&path), |out| {
            let mut pdf = formats::PdfWriter::new(out)?;
            for id in &pages {
                let rgba = reader.straight(*id)?;
                let picture = formats::PdfPicture::new(w, h, &rgba, quality, inks.as_ref());
                drop(rgba);
                pdf.page(&picture, p.doc.dpi, &icc)?;
            }
            pdf.finish().map(|_| ())
        })?;
        self.exported(1.0);
        Ok(())
    }

    /// Each layer as a PNG in `dir` (made if missing), bottom first, named after it; returns
    /// their paths.
    pub fn export_layer_pngs(&self, dir: String) -> Result<Vec<String>, MobileError> {
        self.write_layer_pngs(dir, |_, _| true)
    }

    /// The current and picked layers (a group: the layers in it) each as a PNG in folder `dir`;
    /// returns the files' paths.
    pub fn export_chosen_pngs(&self, dir: String) -> Result<Vec<String>, MobileError> {
        self.write_layer_pngs(dir, |doc, id| doc.chosen().iter().any(|c| *c == id || doc.is_inside(id, *c)))
    }

    pub fn name(&self) -> String {
        self.shared.lock().name.clone()
    }

    /// When the artwork was made, its strokes, the time spent on it (up to now) and its author.
    pub fn artwork_info(&self) -> ArtworkInfo {
        let mut st = self.shared.lock();
        st.count_tracked_time();
        st.info.clone()
    }

    /// Time on the artwork counts while `on` (it is shown and the app in front), from now; off,
    /// what was counted is added to the artwork's. Each save and [`Self::artwork_info`] takes the
    /// time counted so far. (Android counts it itself, by [`Self::add_tracked_time`].)
    pub fn set_tracking(&self, on: bool) {
        let mut st = self.shared.lock();
        st.count_tracked_time();
        if !on {
            st.tracking_since = None;
        } else if st.tracking_since.is_none() {
            st.tracking_since = Some(std::time::Instant::now());
        }
    }

    pub fn set_author(&self, name: String) {
        self.shared.lock().info.author = name.trim().chars().take(256).collect();
    }

    /// Adds time spent on the artwork (at most a day at once), milliseconds.
    pub fn add_tracked_time(&self, ms: u64) {
        let mut st = self.shared.lock();
        st.info.tracked_ms = st.info.tracked_ms.saturating_add(ms.min(24 * 3600 * 1000));
    }

    pub fn set_name(&self, name: String) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        self.shared.lock().name = name.chars().take(256).collect();
        true
    }
}

/// Several artworks as one PDF (a gallery's stack, or artworks picked, shared as a PDF): a page
/// for each, in the order they are added, each its artwork's flattened picture at its own size,
/// resolution and colours. Written beside its place a page at a time and put there by
/// [`PdfBook::finish`]; dropped unfinished, nothing is left.
#[derive(uniffi::Object)]
pub struct PdfBook {
    path: PathBuf,
    quality: u8,
    /// The file being written and its writer, until finished.
    writing: parking_lot::Mutex<Option<(PathBuf, formats::PdfWriter<std::io::BufWriter<fs::File>>)>>,
}

#[uniffi::export]
impl PdfBook {
    /// A PDF to be put at `path`, its pictures kept exactly at `quality` 100, as JPEG below it.
    #[uniffi::constructor]
    pub fn create(path: String, quality: u8) -> Result<Arc<PdfBook>, MobileError> {
        let path = PathBuf::from(path);
        let tmp = beside(&path);
        let file = fs::File::create(&tmp).map_err(|e| file_error("saving", e))?;
        let writer = formats::PdfWriter::new(std::io::BufWriter::new(file)).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            file_error("saving", e)
        })?;
        Ok(Arc::new(PdfBook { path, quality: quality.min(100), writing: parking_lot::Mutex::new(Some((tmp, writer))) }))
    }

    /// The flattened picture of the artwork open in `session` (made ready to draw offscreen) as
    /// the next page.
    pub fn add(&self, session: Arc<CanvasSession>) -> Result<(), MobileError> {
        let (p, _) = session.picture()?;
        let (icc, inks) = file_colours(&p.doc);
        let picture = formats::PdfPicture::new(p.width, p.height, &p.composite, self.quality, inks.as_ref());
        let dpi = p.doc.dpi;
        drop(p);
        let mut writing = self.writing.lock();
        let Some((_, writer)) = writing.as_mut() else {
            return Err(file_error("saving", "the PDF is finished"));
        };
        writer.page(&picture, dpi, &icc).map_err(|e| file_error("saving", e))?;
        session.exported(1.0);
        Ok(())
    }

    /// How many pages it has.
    pub fn pages(&self) -> u32 {
        self.writing.lock().as_ref().map_or(0, |(_, w)| w.pages() as u32)
    }

    /// Ends the PDF and puts it in its place.
    pub fn finish(&self) -> Result<(), MobileError> {
        let Some((tmp, writer)) = self.writing.lock().take() else {
            return Err(file_error("saving", "the PDF is finished"));
        };
        let ended = writer.finish().and_then(|mut out| {
            use std::io::Write;
            out.flush()?;
            out.into_inner().map_err(std::io::IntoInnerError::into_error)?.sync_all()
        });
        if let Err(e) = ended
            .map_err(|e| file_error("saving", e))
            .and_then(|()| fs::rename(&tmp, &self.path).map_err(|e| file_error("saving", e)))
        {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        Ok(())
    }
}

impl Drop for PdfBook {
    fn drop(&mut self) {
        if let Some((tmp, writer)) = self.writing.get_mut().take() {
            drop(writer);
            let _ = fs::remove_file(tmp);
        }
    }
}

/// The signature kept in the artwork's folder `dir` (about/signature.json): its strokes, each the
/// x, y of its points across the signature's box (0..1); none if there is none.
#[uniffi::export]
pub fn read_signature(dir: String) -> Vec<Vec<f32>> {
    let file = Path::new(&dir).join("about").join("signature.json");
    let read: Option<Vec<Vec<f32>>> = fs::read(file).ok().and_then(|b| serde_json::from_slice(&b).ok());
    read.unwrap_or_default()
}

/// Keeps `strokes` as the signature in the artwork's folder `dir` (see [`read_signature`]).
#[uniffi::export]
pub fn write_signature(dir: String, strokes: Vec<Vec<f32>>) -> Result<(), MobileError> {
    let about = Path::new(&dir).join("about");
    fs::create_dir_all(&about).map_err(|e| file_error("keeping the signature", e))?;
    let json = serde_json::to_string(&strokes).map_err(|e| file_error("keeping the signature", e))?;
    write_atomically(&about.join("signature.json"), |p| {
        crate::storage::write(p, &json).map_err(|e| file_error("keeping the signature", e))
    })
}

/// Keeps the picture `bytes` as the author's photo in the artwork's folder `dir`, in the old
/// one's place only once written whole.
#[uniffi::export]
pub fn write_photo(dir: String, bytes: Vec<u8>) -> Result<(), MobileError> {
    let about = Path::new(&dir).join("about");
    fs::create_dir_all(&about).map_err(|e| file_error("keeping the photo", e))?;
    write_atomically(&about.join("photo.jpg"), |p| {
        crate::storage::write(p, &bytes).map_err(|e| file_error("keeping the photo", e))
    })
}

/// The table showing a CMYK canvas's colours as its press prints them (in Display P3, as its
/// pictures carry), if it is one.
fn proof_table(doc: &Document) -> Option<procraft_engine::cms::Lut> {
    use procraft_engine::cms::{Lut, builtin};
    let press = doc.press()?;
    Some(Lut::build(&doc.rgb_profile(), &builtin(procraft_engine::profile::Profile::DisplayP3), Some(&press), 33))
}

/// What a layered or print file of the canvas carries: the profile, and for a CMYK canvas the
/// table to its press's inks.
fn file_colours(doc: &Document) -> (Vec<u8>, Option<procraft_engine::cms::Lut>) {
    match doc.press() {
        Some(press) => {
            let inks = procraft_engine::cms::Lut::build(&doc.rgb_profile(), &press, None, 33);
            (press.bytes.to_vec(), Some(inks))
        }
        None => (doc.picture_icc(), None),
    }
}

/// What an export reads from the canvas first: the flattened picture (straight alpha) and the
/// document. The layers are read as the file is written (see [`ExportLayers`]).
struct Pictures {
    width: u32,
    height: u32,
    doc: Document,
    composite: Vec<u8>,
}

impl CanvasSession {
    /// The layers `wanted` picks out each as a PNG in folder `dir`; returns the files' paths.
    fn write_layer_pngs(
        &self,
        dir: String,
        wanted: impl Fn(&Document, LayerId) -> bool,
    ) -> Result<Vec<String>, MobileError> {
        let (p, edits) = self.picture()?;
        drop(p.composite);
        let icc = p.doc.picture_icc();
        let proof = proof_table(&p.doc);
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir).map_err(|e| file_error("making the folder", e))?;
        let mut paths = Vec::new();
        let mut used = std::collections::HashSet::new();
        // each read and written in turn
        let chosen: Vec<&procraft_engine::document::Layer> =
            p.doc.layers().iter().filter(|l| l.kind == LayerKind::Paint && wanted(&p.doc, l.id)).collect();
        let reader = ExportLayers::new(self, edits, chosen.len(), (0.1, 1.0));
        for l in chosen {
            let rgba = &reader.straight(l.id)?;
            let base: String =
                l.name.chars().map(|c| if c.is_alphanumeric() || " _-".contains(c) { c } else { '_' }).collect();
            let base = if base.trim().is_empty() { "Layer".to_owned() } else { base.trim().to_owned() };
            let mut name = base.clone();
            let mut n = 2;
            while !used.insert(name.clone()) {
                name = format!("{base} {n}");
                n += 1;
            }
            let path = dir.join(format!("{name}.png"));
            let proofed = proof.as_ref().map(|table| {
                let mut px = rgba.clone();
                table.apply_rgba(&mut px, false);
                px
            });
            write_atomically(&path, |f| write_png(f, p.width, p.height, proofed.as_ref().unwrap_or(rgba), &icc))?;
            paths.push(path.to_string_lossy().into_owned());
        }
        self.exported(1.0);
        Ok(paths)
    }

    /// The flattened picture an export writes (straight alpha), with the document as it is and
    /// its edits then (see [`ExportLayers`]).
    fn picture(&self) -> Result<(Pictures, u64), MobileError> {
        self.exported(0.0);
        let mut guard = self.shared.lock_finished();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else {
            return Err(file_error("reading the picture", "the canvas is not shown yet"));
        };
        canvas.render_finished(gpu, &mut st.doc);
        let mut composite = canvas.read_composite(gpu).map_err(|e| file_error("reading the picture", e))?;
        straighten(&mut composite);
        self.exported(0.1);
        let (w, h) = (st.doc.width(), st.doc.height());
        Ok((Pictures { width: w, height: h, doc: st.doc.clone(), composite }, st.edits()))
    }
}

/// Where a PSD's layer's pixels are read from as it is written: a paint layer (and its mask, if
/// it has one shown), or the background colour.
enum PsdFrom {
    Layer(LayerId, Option<LayerId>),
    Background,
    /// A group's start or end: no pixels.
    Nothing,
}

impl PsdFrom {
    /// How many layers the canvas reads back for it.
    fn reads(&self) -> usize {
        match self {
            PsdFrom::Layer(_, mask) => 1 + usize::from(mask.is_some()),
            _ => 0,
        }
    }
}

/// The PSD's layers, top first: groups as a start, their layers and an end; masks with their
/// layers; the background colour, when shown, at the bottom. Their pixels are read as the file is
/// written, from where the second list says (by place).
fn psd_layers(doc: &Document) -> (Vec<PsdLayer<'_>>, Vec<PsdFrom>) {
    let end = || PsdLayer {
        name: "",
        kind: PsdKind::GroupEnd,
        opacity: 1.0,
        visible: true,
        blend: BlendMode::Normal,
        clipped: false,
    };
    let (mut out, mut from) = (Vec::new(), Vec::new());
    let mut open: Vec<LayerId> = Vec::new();
    for l in doc.layers().iter().rev() {
        if l.kind == LayerKind::Mask {
            continue;
        }
        while let Some(&g) = open.last() {
            if doc.is_inside(l.id, g) {
                break;
            }
            open.pop();
            out.push(end());
            from.push(PsdFrom::Nothing);
        }
        let kind = match l.kind {
            LayerKind::Group => {
                open.push(l.id);
                from.push(PsdFrom::Nothing);
                PsdKind::GroupStart { open: !l.collapsed }
            }
            _ => {
                let mask = doc.mask_of(l.id).filter(|m| doc.layer(*m).is_some_and(|m| m.visible));
                from.push(PsdFrom::Layer(l.id, mask));
                PsdKind::Read { masked: mask.is_some() }
            }
        };
        out.push(PsdLayer {
            name: &l.name,
            kind,
            opacity: l.opacity,
            visible: l.visible,
            blend: l.blend,
            clipped: l.clipped,
        });
    }
    for _ in open {
        out.push(end());
        from.push(PsdFrom::Nothing);
    }
    if doc.background_visible {
        out.push(PsdLayer {
            name: "Background",
            kind: PsdKind::Read { masked: false },
            opacity: 1.0,
            visible: true,
            blend: BlendMode::Normal,
            clipped: false,
        });
        from.push(PsdFrom::Background);
    }
    (out, from)
}

/// Reads an open artwork's layers for an export as its file is written, a layer at a time, the
/// session held only while each is read back: an artwork of many layers is never all in memory
/// at once, and nothing waits on the whole export. A layer read after the artwork changed fails
/// the export (its file would show the artwork partly as it was and partly as it is).
struct ExportLayers<'a> {
    session: &'a CanvasSession,
    /// The artwork's edits when the export began.
    edits: u64,
    /// How many layers it reads, how many it has, and the share of the export's progress their
    /// reading takes (from, to).
    count: usize,
    done: std::cell::Cell<usize>,
    progress: (f32, f32),
}

impl<'a> ExportLayers<'a> {
    fn new(session: &'a CanvasSession, edits: u64, count: usize, progress: (f32, f32)) -> ExportLayers<'a> {
        ExportLayers { session, edits, count, done: std::cell::Cell::new(0), progress }
    }

    /// Layer `id`'s pixels as the canvas has them (premultiplied RGBA8, the canvas's size).
    fn premultiplied(&self, id: LayerId) -> Result<Vec<u8>, MobileError> {
        let pixels = {
            let mut guard = self.session.shared.lock();
            let st = &mut *guard;
            if st.edits() != self.edits {
                return Err(file_error("reading a layer", "the artwork changed while it was exported"));
            }
            let (Some(canvas), Some(gpu)) = (st.canvas.as_ref(), st.gpu.as_ref()) else {
                return Err(file_error("reading a layer", "the canvas is not shown yet"));
            };
            canvas.read_layer(gpu, id).map_err(|e| file_error("reading a layer", e))?
        };
        let done = self.done.get() + 1;
        self.done.set(done);
        let (from, to) = self.progress;
        self.session.exported(from + (to - from) * done.min(self.count) as f32 / self.count.max(1) as f32);
        Ok(pixels)
    }

    /// Layer `id`'s pixels with straight alpha.
    fn straight(&self, id: LayerId) -> Result<Vec<u8>, MobileError> {
        let mut pixels = self.premultiplied(id)?;
        straighten(&mut pixels);
        Ok(pixels)
    }

    /// Mask `id`'s grey, a byte a pixel.
    fn grey(&self, id: LayerId) -> Result<Vec<u8>, MobileError> {
        let rgba = self.premultiplied(id)?;
        Ok(rgba.as_chunks::<4>().0.iter().map(|&p| procraft_engine::document::mask_grey(p)).collect())
    }
}

impl procraft_engine::native::LayerPixels for ExportLayers<'_> {
    fn pixels(&self, id: LayerId) -> std::io::Result<Option<std::borrow::Cow<'_, [u8]>>> {
        Ok(Some(std::borrow::Cow::Owned(self.premultiplied(id)?)))
    }
}

impl From<MobileError> for std::io::Error {
    fn from(e: MobileError) -> std::io::Error {
        std::io::Error::other(e.to_string())
    }
}

/// Writes `path` with `write`, given the file (buffered) to write into, as [`write_atomically`]
/// does: a file that couldn't be written whole is never left there.
fn write_file_atomically(
    path: &Path,
    write: impl FnOnce(&mut std::io::BufWriter<crate::storage::File>) -> std::io::Result<()>,
) -> Result<(), MobileError> {
    write_atomically(path, |tmp| {
        let file = crate::storage::create(tmp).map_err(|e| file_error("saving", e))?;
        let mut out = std::io::BufWriter::new(file);
        write(&mut out).map_err(|e| file_error("saving", e))?;
        out.into_inner().map(drop).map_err(|e| file_error("saving", e.into_error()))
    })
}

fn layer_path(dir: &Path, id: LayerId) -> PathBuf {
    dir.join("layers").join(format!("{}.png", id.0))
}

/// Where a plugin's layer's data is kept, beside its pixels.
pub(crate) fn layer_data_path(dir: &Path, id: LayerId) -> PathBuf {
    dir.join("layers").join(format!("{}.layerdata", id.0))
}

/// Where earlier versions kept it: read when the file above isn't there, and left until a save
/// writes that.
fn old_layer_data_path(dir: &Path, id: LayerId) -> PathBuf {
    dir.join("layers").join(format!("{}.vector", id.0))
}

/// What the apps say when saving an artwork failed for `reason` (a save's error).
#[uniffi::export]
pub fn save_failure_said(reason: String) -> String {
    if crate::storage::says_full(&reason) {
        crate::storage::NO_ROOM.to_owned()
    } else {
        "This artwork couldn't be saved.".to_owned()
    }
}

/// Writes `path` by writing a temporary file beside it and renaming it into place.
pub(crate) fn write_atomically(
    path: &Path,
    write: impl FnOnce(&Path) -> Result<(), MobileError>,
) -> Result<(), MobileError> {
    let tmp = write_beside(path, write)?;
    fs::rename(&tmp, path).map_err(|e| file_error("saving", e))
}

/// Writes what goes in `path` into a file beside it (removed again if the write fails), to be
/// renamed over it; the file written.
pub(crate) fn write_beside(
    path: &Path,
    write: impl FnOnce(&Path) -> Result<(), MobileError>,
) -> Result<PathBuf, MobileError> {
    let tmp = beside(path);
    if let Err(e) = write(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(tmp)
}

/// A file beside `path` to write what goes there into: a name of its own, so that no other write
/// of the same file can touch it.
fn beside(path: &Path) -> PathBuf {
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    path.with_extension(format!("{}.tmp", n))
}

/// A PNG of RGBA8 pixels, carrying the colour profile `icc` (none if empty).
pub(crate) fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8], icc: &[u8]) -> Result<(), MobileError> {
    let bytes = png_bytes(width, height, rgba, icc)?;
    crate::storage::write(path, bytes).map_err(|e| file_error("saving", e))
}

/// A PNG of premultiplied RGBA8 pixels (as the GPU gives them), none on failure but empty.
pub(crate) fn rgba_png(width: u32, height: u32, premultiplied: &[u8]) -> Vec<u8> {
    png_bytes(width, height, &straight_alpha(premultiplied), &[]).unwrap_or_default()
}

/// A PNG of RGBA8 pixels as they are (a layer's store's plane).
pub(crate) fn png_of(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, MobileError> {
    png_bytes(width, height, rgba, &[])
}

/// The RGBA8 pixels of the PNG `bytes`, which must be `width` × `height`.
pub(crate) fn png_pixels(bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>, MobileError> {
    decode_png(std::io::Cursor::new(bytes), width, height)
}

/// A PNG of straight RGBA8 pixels, carrying `icc` if it isn't empty.
fn png_bytes(width: u32, height: u32, rgba: &[u8], icc: &[u8]) -> Result<Vec<u8>, MobileError> {
    let mut out = Vec::new();
    let mut info = png::Info::with_size(width, height);
    info.color_type = png::ColorType::Rgba;
    info.bit_depth = png::BitDepth::Eight;
    if !icc.is_empty() {
        info.icc_profile = Some(std::borrow::Cow::Borrowed(icc));
    }
    {
        let mut encoder = png::Encoder::with_info(&mut out, info).map_err(|e| file_error("saving", e))?;
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().map_err(|e| file_error("saving", e))?;
        writer.write_image_data(rgba).map_err(|e| file_error("saving", e))?;
        writer.finish().map_err(|e| file_error("saving", e))?;
    }
    Ok(out)
}

/// A layer's pixels waiting for the GPU: as they are, or (an opened artwork's) still its PNG file,
/// read and decoded only as each goes up, so a large artwork isn't all in memory at once.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Opened {
    Pixels(Vec<u8>),
    File(PathBuf),
    /// Its PNG file's bytes, read before its folder went (see [`CanvasSession::hold_opened_files`]).
    Png(Vec<u8>),
}

impl Opened {
    /// The pixels, `width` × `height`: a layer's file of another size (one a crop's save left as
    /// it was) comes in from its top left corner, cut or with clear pixels to fill, rather than
    /// be lost.
    pub(crate) fn pixels(self, width: u32, height: u32) -> Result<Vec<u8>, MobileError> {
        match self {
            Opened::Pixels(rgba) => Ok(rgba),
            Opened::File(path) => {
                // (tried twice: the file was there)
                let file = fs::File::open(&path)
                    .or_else(|_| fs::File::open(&path))
                    .map_err(|e| file_error("opening a layer", e))?;
                decode_png_into(BufReader::new(file), width, height, true)
            }
            Opened::Png(bytes) => decode_png_into(std::io::Cursor::new(bytes), width, height, true),
        }
    }
}

impl CanvasSession {
    /// Reads the layers' files the artwork was opened from into memory, as they are (packed), so
    /// that their folder can go before the canvas is made: an artwork opened from a folder of the
    /// moment (a file brought in and opened at once). A file that can't be read stays where it is
    /// (it is told, and shows empty, when the canvas is made).
    pub fn hold_opened_files(&self) {
        let mut st = self.shared.lock();
        for (_, layer) in &mut st.pending_pixels {
            if let Opened::File(path) = layer
                && let Ok(bytes) = fs::read(&*path)
            {
                *layer = Opened::Png(bytes);
            }
        }
    }
}

/// A layer's PNG `bytes` decoded as [`Opened::pixels`] decodes its file.
#[cfg(test)]
pub(crate) fn layer_png(bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>, MobileError> {
    decode_png_into(std::io::Cursor::new(bytes), width, height, true)
}

/// Layers decoded on a few threads, `read(i)` making layer `i`'s, and handed to `put` in order on
/// this thread, `at_once` of them held at most (being read, or read and waiting for one before
/// them to be put): an artwork of many layers is never all in memory at once. `put` returning
/// false stops the reading (what is being read is let go).
pub(crate) fn read_in_order<T: Send>(
    count: usize,
    at_once: usize,
    read: impl Fn(usize) -> T + Sync,
    mut put: impl FnMut(usize, T) -> bool,
) {
    let at_once = at_once.max(1);
    let workers =
        std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).clamp(1, 6).min(at_once);
    // how many more may be held, and whether the reading stops
    let room = (parking_lot::Mutex::new((at_once, false)), parking_lot::Condvar::new());
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let (send, receive) = std::sync::mpsc::channel();
        for _ in 0..workers {
            let (send, room, next, read) = (send.clone(), &room, &next, &read);
            scope.spawn(move || {
                loop {
                    // room for a layer first, then the next layer: those held are always the
                    // first not yet put, so the one put next never waits for room
                    {
                        let mut r = room.0.lock();
                        while r.0 == 0 && !r.1 {
                            room.1.wait(&mut r);
                        }
                        if r.1 {
                            break;
                        }
                        r.0 -= 1;
                    }
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= count || send.send((i, read(i))).is_err() {
                        break;
                    }
                }
            });
        }
        drop(send);
        /// Stops the readers once the putting ends, however it ends.
        struct Stop<'a>(&'a (parking_lot::Mutex<(usize, bool)>, parking_lot::Condvar));
        impl Drop for Stop<'_> {
            fn drop(&mut self) {
                self.0.0.lock().1 = true;
                self.0.1.notify_all();
            }
        }
        let _stop = Stop(&room);
        let mut waiting = std::collections::BTreeMap::new();
        let mut at = 0;
        for (i, item) in receive {
            waiting.insert(i, item);
            while let Some(item) = waiting.remove(&at) {
                let go_on = put(at, item);
                at += 1;
                // let go of: another may be read
                room.0.lock().0 += 1;
                room.1.notify_one();
                if !go_on {
                    return;
                }
            }
        }
    });
}

/// Layers' pixels as they are, waiting for the GPU.
pub(crate) fn opened(layers: Vec<(LayerId, Vec<u8>)>) -> Vec<(LayerId, Opened)> {
    layers.into_iter().map(|(id, rgba)| (id, Opened::Pixels(rgba))).collect()
}

/// Waiting layers' pixels, `width` × `height`, and the layers that couldn't be read (left out).
pub(crate) fn opened_pixels(
    layers: Vec<(LayerId, Opened)>,
    width: u32,
    height: u32,
) -> (Vec<(LayerId, Vec<u8>)>, Vec<LayerId>) {
    let mut unread = Vec::new();
    let pixels = layers
        .into_iter()
        .filter_map(|(id, layer)| match layer.pixels(width, height) {
            Ok(rgba) => Some((id, rgba)),
            Err(e) => {
                log::error!("layer {id:?}: {e}");
                unread.push(id);
                None
            }
        })
        .collect();
    (pixels, unread)
}

/// The layers named `names` couldn't be read, as the app tells it.
pub(crate) fn unreadable_said(names: &[String]) -> String {
    match names {
        [one] => format!("The layer “{one}” couldn't be read and shows empty. Its file is kept as it was."),
        _ => format!(
            "{} layers couldn't be read and show empty: {}. Their files are kept as they were.",
            names.len(),
            names.join(", ")
        ),
    }
}

/// Where a layer's file that couldn't be read is kept once the layer is painted on: beside the
/// layers (whose folder a save tidies), never written over.
pub(crate) fn unreadable_path(dir: &Path, id: LayerId) -> PathBuf {
    dir.join(format!("unreadable-{}.png", id.0))
}

/// A PNG's RGBA8 pixels, which must be `width` × `height`.
pub(crate) fn read_png(path: &Path, width: u32, height: u32) -> Result<Vec<u8>, MobileError> {
    let file = fs::File::open(path).map_err(|e| file_error("opening a layer", e))?;
    decode_png(BufReader::new(file), width, height)
}

/// The RGBA8 pixels of the PNG `from` reads, which must be `width` × `height`.
fn decode_png(from: impl std::io::BufRead + std::io::Seek, width: u32, height: u32) -> Result<Vec<u8>, MobileError> {
    decode_png_into(from, width, height, false)
}

/// As [`decode_png`]; `fit`, a PNG of another size is taken from its top left corner into
/// `width` × `height` (cut, or clear where it is smaller).
fn decode_png_into(
    from: impl std::io::BufRead + std::io::Seek,
    width: u32,
    height: u32,
    fit: bool,
) -> Result<Vec<u8>, MobileError> {
    let decoder = png::Decoder::new(from);
    let mut reader = decoder.read_info().map_err(|e| file_error("opening a layer", e))?;
    let (color, depth) = reader.output_color_type();
    let (w, h, interlaced) = (reader.info().width, reader.info().height, reader.info().interlaced);
    let sized = w == width && h == height;
    if (!sized && !fit) || color != png::ColorType::Rgba || depth != png::BitDepth::Eight {
        return Err(file_error("opening a layer", format!("{w}×{h} {color:?}")));
    }
    if !sized {
        log::warn!("a layer of {w}×{h} opened into {width}×{height}, from its top left");
    }
    let line = width as usize * 4;
    let mut buf = vec![0; line * height as usize];
    if interlaced {
        // not one of ours: whole or not at all
        let mut whole = vec![0; w as usize * 4 * h as usize];
        reader.next_frame(&mut whole).map_err(|e| file_error("opening a layer", e))?;
        for (row, from) in buf.chunks_mut(line).zip(whole.chunks(w as usize * 4)) {
            for (to, from) in row.iter_mut().zip(from) {
                *to = *from;
            }
        }
        return Ok(buf);
    }
    // row by row: one damaged part way keeps the rows before the damage (the rest clear) rather
    // than the whole layer being lost
    for (y, row) in buf.chunks_mut(line).enumerate().take(h as usize) {
        match reader.next_row() {
            Ok(Some(read)) => {
                for (to, from) in row.iter_mut().zip(read.data()) {
                    *to = *from;
                }
            }
            Ok(None) => break,
            Err(e) if y == 0 => return Err(file_error("opening a layer", e)),
            Err(e) => {
                log::error!("a layer read as far as row {y}: {e}");
                break;
            }
        }
    }
    Ok(buf)
}

/// Premultiplied RGBA8 pixels with straight alpha instead.
impl CanvasSession {
    /// The flattened picture as it is exported, full size: its width, height, pixels (straight
    /// RGBA8; a CMYK canvas's as its press prints them) and the colour profile they are in.
    fn finished_picture(&self) -> Result<(u32, u32, Vec<u8>, Vec<u8>), MobileError> {
        self.exported(0.0);
        let (w, h, rgba, icc) = {
            let mut guard = self.shared.lock_finished();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            let (w, h) = (st.doc.width(), st.doc.height());
            let icc = st.doc.picture_icc();
            let proof = proof_table(&st.doc);
            let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else {
                return Err(file_error("reading the picture", "the canvas is not shown yet"));
            };
            canvas.render_finished(gpu, &mut st.doc);
            let rgba = canvas.read_composite(gpu).map_err(|e| file_error("reading the picture", e))?;
            (w, h, rgba, (icc, proof))
        };
        let (icc, proof) = icc;
        let mut straight = rgba;
        straighten(&mut straight);
        if let Some(table) = proof {
            table.apply_rgba(&mut straight, false);
        }
        Ok((w, h, straight, icc))
    }
}

/// What an artwork called `name` is called as a file it is exported to, before the extension:
/// its letters, digits, spaces, `_` and `-` (the rest left out, the ends trimmed), or "Untitled
/// Artwork" with none left; a layered package's (`package`) with underscores for its spaces.
#[uniffi::export]
pub fn export_file_name(name: String, package: bool) -> String {
    let kept: String = name.chars().filter(|c| c.is_alphanumeric() || " _-".contains(*c)).collect();
    let base = match kept.trim() {
        "" => "Untitled Artwork",
        k => k,
    };
    if package { base.replace(' ', "_") } else { base.to_owned() }
}

/// How finely JPEGs are written (1..100): fine enough to keep a painting's edges and grain.
pub(crate) const JPEG_QUALITY: u8 = 92;

/// Straight RGBA8 pixels laid onto white, as RGB8 (for a JPEG, which has nothing see-through); an
/// error if there is no memory for them (a big canvas's, on a computer short of it), not a crash.
pub(crate) fn onto_white(rgba: &[u8]) -> Result<Vec<u8>, MobileError> {
    let (pixels, _) = rgba.as_chunks::<4>();
    let mut out = Vec::new();
    out.try_reserve_exact(pixels.len() * 3).map_err(|e| file_error("making the picture", e))?;
    for &[r, g, b, a] in pixels {
        let a = u16::from(a);
        let over = |c: u8| ((u16::from(c) * a + 255 * (255 - a) + 127) / 255) as u8;
        out.extend_from_slice(&[over(r), over(g), over(b)]);
    }
    Ok(out)
}

/// [`straight_alpha`] in place, in pieces made side by side.
pub(crate) fn straighten(rgba: &mut [u8]) {
    procraft_engine::par::each_part(rgba, 1 << 20, |piece| {
        for px in piece.as_chunks_mut::<4>().0 {
            let [r, g, b, a] = *px;
            let alpha = u32::from(a);
            let un = |c: u8| (u32::from(c) * 255 + alpha / 2).checked_div(alpha).map_or(0, |v| v.min(255) as u8);
            *px = [un(r), un(g), un(b), a];
        }
    });
}

pub(crate) fn straight_alpha(rgba: &[u8]) -> Vec<u8> {
    // in pieces made side by side
    let pieces: Vec<&[u8]> = rgba.chunks(1 << 20).collect();
    let made = procraft_engine::par::side_by_side(&pieces, |piece| straight_piece(piece));
    let mut out = Vec::with_capacity(rgba.len());
    for (piece, made) in pieces.iter().zip(made) {
        out.extend_from_slice(&made.unwrap_or_else(|| straight_piece(piece)));
    }
    out
}

/// [`straight_alpha`] of one piece (whole pixels).
fn straight_piece(rgba: &[u8]) -> Vec<u8> {
    let (pixels, _) = rgba.as_chunks::<4>();
    let mut out = Vec::with_capacity(rgba.len());
    for &[r, g, b, a] in pixels {
        let alpha = u32::from(a);
        let un = |c: u8| (u32::from(c) * 255 + alpha / 2).checked_div(alpha).map_or(0, |v| v.min(255) as u8);
        out.extend_from_slice(&[un(r), un(g), un(b), a]);
    }
    out
}

/// How bringing in a document's pages went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PagesOutcome {
    Imported,
    /// More pages than a canvas their size holds as layers (`most`).
    TooManyPages {
        most: u32,
    },
    /// A page couldn't be read.
    Unreadable,
}

/// Makes an artwork named `name` in the folder `dir` from `count` pages, each a `width` x `height`
/// PNG `page-N.png` (N from 1) in the folder `pages`: a layer each, "Page N", the first at the
/// bottom, with Page Assist on and `dpi` its resolution. Used for a PDF's pages.
#[uniffi::export]
pub fn import_pages(
    pages: String,
    count: u32,
    width: u32,
    height: u32,
    dpi: f32,
    dir: String,
    name: String,
) -> PagesOutcome {
    use procraft_engine::document::Layer;
    let most = procraft_engine::document::canvas_edge_most();
    let (w, h) = (width.clamp(1, most), height.clamp(1, most));
    let most = Document::new(w, h).layer_cap();
    if count as usize > most {
        return PagesOutcome::TooManyPages { most: u32::try_from(most).unwrap_or(u32::MAX) };
    }
    let mut layers = Vec::new();
    let mut pixels = Vec::new();
    for n in 1..=count.max(1) {
        let id = LayerId(u64::from(n));
        match read_png(&Path::new(&pages).join(format!("page-{n}.png")), w, h) {
            Ok(rgba) => pixels.push((id, rgba)),
            Err(e) => {
                log::error!("a page: {e}");
                return PagesOutcome::Unreadable;
            }
        }
        layers.push(Layer {
            id,
            name: format!("Page {n}"),
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            alpha_lock: false,
            clipped: false,
            locked: false,
            reference: false,
            kind: LayerKind::Paint,
            parent: None,
            collapsed: false,
            text: None,
            private: false,
            assisted: false,
            plugin: None,
            filter: None,
            marks: Default::default(),
        });
    }
    let mut doc = Document::from_layers(w, h, layers, LayerId(1));
    if dpi.is_finite() && dpi > 0.0 {
        doc.dpi = dpi.clamp(1.0, 9999.0);
    }
    doc.animation = Some(procraft_engine::animation::Animation { pages: true, ..Default::default() });
    let name =
        if name.trim().is_empty() { "Untitled Artwork".to_owned() } else { name.trim().chars().take(256).collect() };
    // the first page is the gallery's thumbnail until the artwork is opened
    let first = pixels.first().map(|(_, p)| thumbnail(p, w, h, THUMBNAIL_SIDE));
    // the folder isn't an artwork until it is whole (taken away if the app stops meanwhile)
    let importing = crate::gallery::Importing::begin(Path::new(&dir));
    let session = CanvasSession::with_document(doc, name, pixels);
    if let Err(e) = session.save(dir.clone()) {
        log::error!("saving imported pages: {e}");
        return PagesOutcome::Unreadable;
    }
    if let Some((tw, th, thumb)) = first
        && let Err(e) = write_atomically(&Path::new(&dir).join("thumbnail.png"), |p| write_png(p, tw, th, &thumb, &[]))
    {
        log::error!("imported pages' thumbnail: {e}");
    }
    importing.done();
    PagesOutcome::Imported
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ClipOutcome {
    Imported {
        approximated_blends: u32,
        rasterized_layers: u32,
        unsupported_layers: u32,
        imported_time_lapse_frames: u32,
        unsupported_time_lapse_records: u32,
        unsupported_animation_tracks: u32,
        preserved_vector_layers: u32,
        source_clip_preserved: bool,
    },
    TooManyLayers {
        most: u32,
    },
    TooLarge,
    Unsupported,
    Unreadable,
}

#[uniffi::export]
pub fn import_clip(path: String, dir: String, name: String) -> ClipOutcome {
    let mut artwork = match procraft_engine::clip::ClipArtwork::open(Path::new(&path)) {
        Ok(artwork) => artwork,
        Err(procraft_engine::clip::ClipError::TooLarge) => return ClipOutcome::TooLarge,
        Err(procraft_engine::clip::ClipError::Unsupported(_)) => return ClipOutcome::Unsupported,
        Err(error) => {
            log::error!("opening a CLIP document: {error:?}");
            return ClipOutcome::Unreadable;
        }
    };
    if !artwork.approximated_blends().is_empty() {
        log::warn!("importing a CLIP document approximated {} blend modes", artwork.approximated_blends().len());
    }
    let document = artwork.document().clone();
    let pixel_layers = document.pixel_layer_ids().count();
    if pixel_layers > document.layer_cap() {
        return ClipOutcome::TooManyLayers { most: u32::try_from(document.layer_cap()).unwrap_or(u32::MAX) };
    }
    let importing = crate::gallery::Importing::begin(Path::new(&dir));
    match write_clip_artwork(&mut artwork, Path::new(&dir), name) {
        Ok(()) => {
            let (imported_time_lapse_frames, time_lapse_failures) = write_clip_time_lapse(&artwork, Path::new(&dir));
            let source_size = fs::metadata(&path).map_or(u64::MAX, |metadata| metadata.len());
            let source_clip_preserved = source_size <= 512 * 1024 * 1024
                && write_atomically(&Path::new(&dir).join("source.clip"), |target| {
                    fs::copy(&path, target).map(|_| ()).map_err(|error| file_error("preserving the CLIP source", error))
                })
                .is_ok();
            importing.done();
            ClipOutcome::Imported {
                approximated_blends: u32::try_from(artwork.approximated_blends().len()).unwrap_or(u32::MAX),
                rasterized_layers: artwork.rasterized_layer_count(),
                unsupported_layers: artwork.unsupported_layer_count(),
                imported_time_lapse_frames,
                unsupported_time_lapse_records: artwork
                    .unsupported_time_lapse_records()
                    .saturating_add(time_lapse_failures),
                unsupported_animation_tracks: artwork.unsupported_animation_tracks(),
                preserved_vector_layers: artwork.vector_source_layer_count(),
                source_clip_preserved,
            }
        }
        Err(error) => {
            log::error!("saving an imported CLIP document: {error}");
            ClipOutcome::Unreadable
        }
    }
}

fn write_clip_time_lapse(artwork: &procraft_engine::clip::ClipArtwork, dir: &Path) -> (u32, u32) {
    let frames = artwork.time_lapse_frames();
    if frames.is_empty() {
        return (0, 0);
    }
    let lapse = crate::timelapse::TimeLapse::new(dir.to_string_lossy().into_owned());
    let mut imported = 0_u32;
    let mut failures = 0_u32;
    for frame in frames {
        let decoded = image::load_from_memory_with_format(frame, image::ImageFormat::Png);
        let Ok(mut rgba) = decoded.map(|picture| picture.to_rgba8()) else {
            failures = failures.saturating_add(1);
            continue;
        };
        let (width, height) = rgba.dimensions();
        for pixel in rgba.pixels_mut() {
            let [r, g, b, a] = pixel.0;
            let alpha = u16::from(a);
            *pixel = image::Rgba([
                ((u16::from(r) * alpha + 255 * (255 - alpha) + 127) / 255) as u8,
                ((u16::from(g) * alpha + 255 * (255 - alpha) + 127) / 255) as u8,
                ((u16::from(b) * alpha + 255 * (255 - alpha) + 127) / 255) as u8,
                255,
            ]);
        }
        if lapse.keep_frame(rgba.into_raw(), width, height) {
            imported = imported.saturating_add(1);
        } else {
            failures = failures.saturating_add(1);
            if let Some(problem) = lapse.take_problem() {
                log::warn!("importing a CLIP time-lapse frame: {problem}");
            }
        }
    }
    (imported, failures)
}

fn write_clip_artwork(
    artwork: &mut procraft_engine::clip::ClipArtwork,
    dir: &Path,
    name: String,
) -> Result<(), MobileError> {
    let document = artwork.document().clone();
    let (width, height) = (document.width(), document.height());
    fs::create_dir_all(dir.join("layers")).map_err(|error| file_error("making the folder", error))?;
    let mut files = Vec::new();
    for id in artwork.layers_with_pixels().collect::<Vec<_>>() {
        let rgba = match artwork.pixels(id).map_err(|error| file_error("reading a CLIP layer", format!("{error:?}")))? {
            Some(rgba) => rgba,
            None => match artwork.vector_strokes(id) {
                Some(strokes) => crate::plugin_canvas::rasterise_clip_vectors(strokes, width, height),
                None => continue,
            },
        };
        write_atomically(&layer_path(dir, id), |path| write_png(path, width, height, &rgba, &[]))?;
        files.push((id, Opened::File(layer_path(dir, id))));
    }
    if let Some(preview) = artwork.preview() {
        write_atomically(&dir.join("thumbnail.png"), |path| {
            fs::write(path, preview).map_err(|error| file_error("writing the thumbnail", error))
        })?;
    }
    let name = if name.trim().is_empty() { "Untitled Artwork".to_owned() } else { name };
    let session = CanvasSession::with_document(document, name, Vec::new());
    session.shared.lock().pending_pixels = files;
    session.save(dir.to_string_lossy().into_owned())
}

/// How bringing in a PSD went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PsdOutcome {
    Imported,
    /// More layers than a canvas its size holds (`most`): none of it comes in.
    TooManyLayers {
        most: u32,
    },
    /// A canvas larger than this device holds (its sides, or a layer's memory).
    TooLarge,
    /// A kind of PSD not read here (bitmap, indexed, Lab, 32 bits a channel).
    Unsupported,
    /// Not a PSD, or damaged.
    Unreadable,
}

/// The time-lapse the package at `path` keeps, its segments written into the folder `dir` as
/// `segment-N.mp4` (N from 1, in order); none if it keeps none.
#[uniffi::export]
pub fn package_time_lapse(path: String, dir: String) -> Result<Option<PackageTimeLapse>, MobileError> {
    let bytes = fs::read(&path).map_err(|e| file_error("opening the file", e))?;
    let Some(t) = procraft_engine::native::read_time_lapse(&bytes)
        .map_err(|e| file_error("opening the file", format!("{e:?}")))?
    else {
        return Ok(None);
    };
    let dir = Path::new(&dir);
    if !t.segments.is_empty() {
        fs::create_dir_all(dir).map_err(|e| file_error("saving the time-lapse", e))?;
    }
    let mut segments = Vec::with_capacity(t.segments.len());
    for (i, data) in t.segments.iter().enumerate() {
        let file = dir.join(format!("segment-{}.mp4", i + 1));
        crate::storage::write(&file, data).map_err(|e| file_error("saving the time-lapse", e))?;
        segments.push(file.to_string_lossy().into_owned());
    }
    Ok(Some(PackageTimeLapse {
        recording: t.recording,
        quality: t.quality,
        resolution: t.resolution,
        hevc: t.hevc,
        segments,
        width: t.frame_size.0,
        height: t.frame_size.1,
        fps: t.fps,
        bitrate: t.bitrate,
    }))
}

/// Keeps the preview the layered package at `path` brings (its `QuickLook/Thumbnail.png`) as the
/// gallery's thumbnail of the artwork made of it in the folder `dir`, until that is opened and
/// saved; false if it brings none (or it can't be kept).
#[uniffi::export]
pub fn keep_package_thumbnail(path: String, dir: String) -> bool {
    let Ok(bytes) = fs::read(&path) else { return false };
    let Some(png) = procraft_engine::zip::ZipReader::new(&bytes).and_then(|zip| zip.read(PACKAGE_PREVIEW)) else {
        return false;
    };
    let kept = write_atomically(&Path::new(&dir).join("thumbnail.png"), |p| {
        crate::storage::write(p, &png).map_err(|e| file_error("keeping the package's preview", e))
    });
    if let Err(e) = &kept {
        log::error!("{e}");
    }
    kept.is_ok()
}

/// Where a layered package keeps its preview.
const PACKAGE_PREVIEW: &str = "QuickLook/Thumbnail.png";

/// Makes the layered PSD (or PSB) at `path` an artwork named `name` in the folder `dir`: its
/// layers, groups and masks, and its flattened picture as the gallery's thumbnail until opened.
/// The file is only read, a part at a time where it is needed: each layer is read and written as
/// a file of its own a few at a time, so a file of many layers is never all in memory, and the
/// document goes last, so an import cut short (the app stopped) leaves no artwork in the gallery.
#[uniffi::export]
pub fn import_psd(path: String, dir: String, name: String) -> PsdOutcome {
    use procraft_engine::psd::{self, FileSource, PsdError};
    let file = match FileSource::open(Path::new(&path)) {
        Ok(f) => f,
        Err(e) => {
            log::error!("opening a PSD: {e}");
            return PsdOutcome::Unreadable;
        }
    };
    let art = match psd::open(&file) {
        Ok(a) => a,
        Err(PsdError::TooManyLayers(most)) => {
            return PsdOutcome::TooManyLayers { most: u32::try_from(most).unwrap_or(u32::MAX) };
        }
        Err(PsdError::TooLarge) => return PsdOutcome::TooLarge,
        Err(PsdError::Unsupported(_)) => return PsdOutcome::Unsupported,
        Err(e) => {
            log::error!("reading a PSD: {e:?}");
            return PsdOutcome::Unreadable;
        }
    };
    if art.salvaged {
        log::warn!("a PSD's layers couldn't be read: its flattened picture came in as its one layer");
    }
    log::info!(
        "importing a PSD: {} x {}, {} layers",
        art.document.width(),
        art.document.height(),
        art.document.layers().len()
    );
    // the folder isn't an artwork until it is whole (taken away if the app stops meanwhile)
    let importing = crate::gallery::Importing::begin(Path::new(&dir));
    match write_psd_artwork(&art, Path::new(&dir), name) {
        Ok(()) => {
            importing.done();
            PsdOutcome::Imported
        }
        Err(e) => {
            log::error!("saving an imported PSD: {e}");
            PsdOutcome::Unreadable
        }
    }
}

/// The PSD's artwork written into the folder `dir`, named `name`: each layer read and written whole
/// as it is read (a few side by side, as many as the memory work side by side may hold takes: each
/// holds its pixels, what they are read from, and its file), none for a clear one; the thumbnail;
/// the document last.
fn write_psd_artwork<S: procraft_engine::psd::Source + ?Sized>(
    art: &procraft_engine::psd::PsdArtwork<'_, S>,
    dir: &Path,
    name: String,
) -> Result<(), MobileError> {
    let (w, h) = (art.document.width(), art.document.height());
    fs::create_dir_all(dir.join("layers")).map_err(|e| file_error("making the folder", e))?;
    let ids = art.layers_with_pixels();
    let each = w as usize * h as usize * 4 * 3;
    let written = procraft_engine::par::side_by_side_within(&ids, each, |&id| -> Result<bool, MobileError> {
        let Some(rgba) = art.pixels(id) else { return Ok(false) };
        write_atomically(&layer_path(dir, id), |p| write_png(p, w, h, &rgba, &[]))?;
        Ok(true)
    });
    let mut files = Vec::with_capacity(ids.len());
    for (id, done) in ids.iter().zip(written) {
        match done.unwrap_or_else(|| Err(file_error("writing a layer", "it stopped"))) {
            Ok(true) => files.push((*id, Opened::File(layer_path(dir, *id)))),
            Ok(false) => {}
            Err(e) => return Err(e),
        }
    }
    // the gallery's thumbnail until it is opened: the flattened picture on the white of a new
    // canvas, made small as it is read
    if let Some((tw, th, thumb)) = art.thumbnail(THUMBNAIL_SIDE)
        && let Err(e) = write_atomically(&dir.join("thumbnail.png"), |p| write_png(p, tw, th, &thumb, &[]))
    {
        log::error!("an imported PSD's thumbnail: {e}");
    }
    // the document last, its layers the files just written
    let name = if name.trim().is_empty() { "Untitled Artwork".to_owned() } else { name };
    let session = CanvasSession::with_document(art.document.clone(), name, Vec::new());
    session.shared.lock().pending_pixels = files;
    session.save(dir.to_string_lossy().into_owned())
}

/// The picture scaled down (box filter) to fit `side`, straight alpha, for viewing elsewhere.
/// A picture (`width` x `height`, RGBA) turned `turns` quarter turns clockwise.
fn turned(width: u32, height: u32, rgba: Vec<u8>, turns: u8) -> (u32, u32, Vec<u8>) {
    let turns = u32::from(turns % 4);
    let rgba = procraft_engine::native::orient(rgba, width as usize, height as usize, 4, turns, false, false);
    if turns % 2 == 1 { (height, width, rgba) } else { (width, height, rgba) }
}

pub(crate) fn thumbnail(rgba: &[u8], width: u32, height: u32, side: u32) -> (u32, u32, Vec<u8>) {
    let scale = (side as f32 / width.max(height).max(1) as f32).min(1.0);
    let (tw, th) = (((width as f32 * scale).round() as u32).max(1), ((height as f32 * scale).round() as u32).max(1));
    // at full size each pixel is only made straight
    let whole = width as usize * height as usize * 4;
    if (tw, th) == (width, height)
        && let Some(all) = rgba.get(..whole)
    {
        return (tw, th, straight_alpha(all));
    }
    // each row averaged on its own, side by side
    let rows: Vec<u32> = (0..th).collect();
    let made = procraft_engine::par::side_by_side(&rows, |&ty| thumbnail_row(rgba, width, height, tw, th, ty));
    let mut out = Vec::with_capacity(tw as usize * th as usize * 4);
    for (&ty, row) in rows.iter().zip(made) {
        out.extend_from_slice(&row.unwrap_or_else(|| thumbnail_row(rgba, width, height, tw, th, ty)));
    }
    (tw, th, out)
}

/// Row `ty` of [`thumbnail`]'s `tw` x `th` picture: each pixel the average of the premultiplied
/// pixels it covers, made straight.
fn thumbnail_row(rgba: &[u8], width: u32, height: u32, tw: u32, th: u32, ty: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((tw * 4) as usize);
    {
        let (y0, y1) = (ty * height / th, ((ty + 1) * height / th).max(ty * height / th + 1));
        for tx in 0..tw {
            let (x0, x1) = (tx * width / tw, ((tx + 1) * width / tw).max(tx * width / tw + 1));
            let mut sum = [0u32; 4];
            let mut n = 0u32;
            for y in y0..y1.min(height) {
                for x in x0..x1.min(width) {
                    let i = ((y * width + x) * 4) as usize;
                    if let Some(px) = rgba.get(i..i + 4) {
                        for (s, v) in sum.iter_mut().zip(px) {
                            *s += u32::from(*v);
                        }
                        n += 1;
                    }
                }
            }
            let n = n.max(1);
            let [r, g, b, a] = sum.map(|s| s / n);
            // premultiplied to straight
            let un = |c: u32| (c * 255 + a / 2).checked_div(a).map_or(0, |v| v.min(255) as u8);
            out.extend_from_slice(&[un(r), un(g), un(b), a as u8]);
        }
    }
    out
}

#[cfg(test)]
mod signature_tests {
    use super::*;

    #[test]
    fn a_package_s_preview_is_kept_as_the_thumbnail_until_opened() {
        let root = std::env::temp_dir().join(format!("procraft-package-preview-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let preview: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        let with = root.join("with.procreate");
        let mut zip = procraft_engine::zip::ZipWriter::new();
        zip.add("Document.archive", b"plist");
        zip.add(PACKAGE_PREVIEW, &preview);
        fs::write(&with, zip.finish()).unwrap();
        let without = root.join("without.procreate");
        let mut zip = procraft_engine::zip::ZipWriter::new();
        zip.add("Document.archive", b"plist");
        fs::write(&without, zip.finish()).unwrap();
        let dir = root.join("artwork");
        fs::create_dir_all(&dir).unwrap();
        let path = |p: &Path| p.to_string_lossy().into_owned();
        // a package with a preview: its 1000 bytes, as they are
        assert!(keep_package_thumbnail(path(&with), path(&dir)));
        assert_eq!(fs::read(dir.join("thumbnail.png")).unwrap(), preview);
        // one without, a file that isn't a zip, or none at all: nothing is kept, the thumbnail
        // there before stays
        fs::write(dir.join("thumbnail.png"), [7u8; 3]).unwrap();
        assert!(!keep_package_thumbnail(path(&without), path(&dir)));
        fs::write(root.join("not.zip"), b"not a zip").unwrap();
        assert!(!keep_package_thumbnail(path(&root.join("not.zip")), path(&dir)));
        assert!(!keep_package_thumbnail(path(&root.join("gone.procreate")), path(&dir)));
        assert_eq!(fs::read(dir.join("thumbnail.png")).unwrap(), [7u8; 3]);
        // no file left beside it by the writes
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_autosave_waits_longer_after_a_slow_save() {
        use std::time::Duration;
        let every = Duration::from_secs(3);
        // a quick save: its usual time
        assert_eq!(autosave_wait(every, Duration::from_millis(200)), every);
        assert_eq!(autosave_wait(every, Duration::ZERO), every);
        // a slow one: four times as long as it took, at most half a minute
        assert_eq!(autosave_wait(every, Duration::from_millis(1500)), Duration::from_secs(6));
        assert_eq!(autosave_wait(every, Duration::from_secs(20)), AUTOSAVE_LONGEST);
        assert_eq!(autosave_wait(every, Duration::MAX), AUTOSAVE_LONGEST);
    }

    #[test]
    fn a_layer_file_cut_short_keeps_the_rows_before_the_cut() {
        let (w, h) = (64u32, 64u32);
        // each row its own grey, so a row in the wrong place shows
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let g = (i / w * 4) as u8;
                [g, 255 - g, g, 255]
            })
            .collect();
        let path = std::env::temp_dir().join(format!("procraft-cut-layer-{}.png", std::process::id()));
        write_png(&path, w, h, &rgba, &[]).unwrap();
        let whole = fs::read(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(layer_png(&whole, w, h).unwrap(), rgba);
        // cut short part way: the first rows as they were, the rest clear
        let cut = layer_png(&whole[..whole.len() * 2 / 3], w, h).unwrap();
        let line = w as usize * 4;
        let kept = cut.chunks(line).zip(rgba.chunks(line)).take_while(|(a, b)| a == b).count();
        assert!(kept > 0 && kept < h as usize, "{kept} rows kept");
        assert!(cut[kept * line..].iter().all(|&v| v == 0));
        // cut before any row: none can be read
        assert!(layer_png(&whole[..40], w, h).is_err());
    }

    #[test]
    fn a_layer_file_of_another_size_opens_from_its_top_left() {
        let (w, h) = (6u32, 4u32);
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| [(i % w) as u8 * 40, (i / w) as u8 * 60, 7, 255]).collect();
        let path = std::env::temp_dir().join(format!("procraft-sized-layer-{}.png", std::process::id()));
        write_png(&path, w, h, &rgba, &[]).unwrap();
        let bytes = fs::read(&path).unwrap();
        let _ = fs::remove_file(&path);
        let at = |px: &[u8], cw: u32, x: u32, y: u32| px[((y * cw + x) * 4) as usize..][..4].to_vec();
        // smaller: the top left part
        let cut = layer_png(&bytes, 4, 3).unwrap();
        assert_eq!(cut.len(), 4 * 3 * 4);
        for (x, y) in [(0, 0), (3, 0), (0, 2), (3, 2)] {
            assert_eq!(at(&cut, 4, x, y), at(&rgba, w, x, y));
        }
        // bigger: all of it at the top left, clear around it
        let grown = layer_png(&bytes, 8, 5).unwrap();
        assert_eq!(at(&grown, 8, 5, 3), at(&rgba, w, 5, 3));
        assert_eq!(at(&grown, 8, 6, 0), vec![0; 4]);
        assert_eq!(at(&grown, 8, 0, 4), vec![0; 4]);
        // anything else read by size still must be that size
        assert!(read_png_bytes_sized(&bytes, 4, 3).is_err());
    }

    fn read_png_bytes_sized(bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>, MobileError> {
        decode_png(std::io::Cursor::new(bytes.to_vec()), width, height)
    }

    #[test]
    fn a_signature_is_kept_as_the_android_app_keeps_it() {
        let dir = std::env::temp_dir().join(format!("procraft-signature-{}", std::process::id()));
        let path = dir.to_string_lossy().into_owned();
        assert!(read_signature(path.clone()).is_empty());
        let strokes = vec![vec![0.1, 0.2, 0.3, 0.4], vec![0.5, 0.5]];
        write_signature(path.clone(), strokes.clone()).unwrap();
        assert_eq!(read_signature(path.clone()), strokes);
        // as the Android app wrote it: an array of arrays of numbers
        fs::write(dir.join("about").join("signature.json"), "[[0.25,0.75,1.0,0.0]]").unwrap();
        assert_eq!(read_signature(path), vec![vec![0.25, 0.75, 1.0, 0.0]]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_photo_and_signature_are_replaced_whole_with_nothing_left_beside() {
        let dir = std::env::temp_dir().join(format!("procraft-about-{}", std::process::id()));
        let path = dir.to_string_lossy().into_owned();
        write_photo(path.clone(), b"first photo".to_vec()).unwrap();
        write_signature(path.clone(), vec![vec![0.1, 0.2]]).unwrap();
        write_photo(path.clone(), b"second".to_vec()).unwrap();
        write_signature(path.clone(), vec![vec![0.3, 0.4, 0.5, 0.6]]).unwrap();
        let about = dir.join("about");
        assert_eq!(fs::read(about.join("photo.jpg")).unwrap(), b"second");
        assert_eq!(read_signature(path), vec![vec![0.3, 0.4, 0.5, 0.6]]);
        let mut left: Vec<String> =
            fs::read_dir(&about).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["photo.jpg", "signature.json"]);
        let _ = fs::remove_dir_all(&dir);
    }
}
