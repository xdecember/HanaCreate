use std::{fs::File, io};

use clipfile::{ClipFile, Database, PixelFormat, RasterDataState, VectorStrokeKind};

use crate::document::{BlendMode, Document, Layer, LayerId, LayerKind, layer_budget, layer_room_for};

#[derive(Debug)]
pub enum ClipError {
    Io(io::Error),
    Format(clipfile::Error),
    Unsupported(&'static str),
    TooLarge,
    Invalid(&'static str),
}

impl From<io::Error> for ClipError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<clipfile::Error> for ClipError {
    fn from(error: clipfile::Error) -> Self {
        Self::Format(error)
    }
}

#[derive(Clone, Copy)]
enum PixelSource {
    Layer(i64),
    Mask(i64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipVectorPoint {
    pub position: [f32; 2],
    pub width_factor: f32,
    pub opacity_factor: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipVectorStroke {
    pub points: Vec<ClipVectorPoint>,
    pub color: [u8; 3],
    pub opacity: f32,
    pub width: f32,
    pub closed: bool,
}

pub struct ClipArtwork {
    file: ClipFile<File>,
    database: Database,
    document: Document,
    canvas_id: i64,
    pixel_sources: Vec<(LayerId, PixelSource)>,
    vector_strokes: std::collections::BTreeMap<LayerId, Vec<ClipVectorStroke>>,
    vector_source_layers: u32,
    skipped_raster_layers: std::collections::BTreeSet<LayerId>,
    time_lapse_frames: Vec<Vec<u8>>,
    unsupported_time_lapse_records: u32,
    unsupported_animation_tracks: u32,
    preview: Option<Vec<u8>>,
    approximated_blends: Vec<(i64, i64)>,
    source_blend_modes: Vec<(LayerId, i64)>,
    rasterized_layers: u32,
    unsupported_layers: u32,
}

impl ClipArtwork {
    pub fn open(path: &std::path::Path) -> Result<Self, ClipError> {
        let mut file = ClipFile::open(File::open(path)?)?;
        let source_document = file.read_document()?;
        let canvas_id = source_document
            .project()
            .primary_canvas_id()
            .or_else(|| source_document.canvases().first().map(clipfile::Canvas::id))
            .ok_or(ClipError::Invalid("canvas"))?;
        let canvas = source_document.canvas(canvas_id).ok_or(ClipError::Invalid("canvas"))?;
        if canvas.unit() != 0 {
            return Err(ClipError::Unsupported("non-pixel canvas units"));
        }
        let width = canvas.width().round();
        let height = canvas.height().round();
        if !width.is_finite() || !height.is_finite() || width < 1.0 || height < 1.0 {
            return Err(ClipError::Invalid("canvas dimensions"));
        }
        let width = u32::try_from(width as u64).map_err(|_| ClipError::Invalid("canvas width"))?;
        let height = u32::try_from(height as u64).map_err(|_| ClipError::Invalid("canvas height"))?;
        if width > crate::document::MAX_CANVAS_EDGE
            || height > crate::document::MAX_CANVAS_EDGE
            || layer_room_for(width, height) == 0
        {
            return Err(ClipError::TooLarge);
        }
        let tree = source_document.layer_tree(canvas_id).ok_or(ClipError::Invalid("layer tree"))?;
        let mut layers = Vec::new();
        let mut pixel_sources = Vec::new();
        let mut source_to_target = std::collections::BTreeMap::new();
        let mut approximated_blends = Vec::new();
        let mut rasterized_layers = 0_u32;
        let mut unsupported_layers = 0_u32;
        let mut next_id = 1_u64;
        append_children(
            tree,
            &source_document,
            tree.root_layer_id(),
            None,
            &mut layers,
            &mut pixel_sources,
            &mut source_to_target,
            &mut approximated_blends,
            &mut rasterized_layers,
            &mut unsupported_layers,
            &mut next_id,
        )?;
        let source_blend_modes = source_to_target
            .iter()
            .filter_map(|(source_id, target_id)| {
                source_document.layer(*source_id).map(|layer| (*target_id, layer.blend_mode().raw()))
            })
            .collect::<Vec<_>>();
        let current =
            canvas.current_layer_id().and_then(|id| source_to_target.get(&id).copied()).unwrap_or_else(|| {
                layers.iter().find(|layer| layer.kind == LayerKind::Paint).map_or(LayerId(1), |layer| layer.id)
            });
        let mut document = Document::from_layers(width, height, layers, current);
        if canvas.resolution().is_finite() && canvas.resolution() > 0.0 {
            document.dpi = canvas.resolution() as f32;
        }
        let database = file.open_database()?;
        let mut time_lapse_frames = Vec::new();
        let mut unsupported_time_lapse_records = 0_u32;
        match database.time_lapse(file.limits()) {
            Ok(Some(time_lapse)) => {
                for manager in time_lapse.managers() {
                    for record in manager.records() {
                        if record.encoder_name() != "V2" {
                            unsupported_time_lapse_records = unsupported_time_lapse_records.saturating_add(1);
                            continue;
                        }
                        let mut payload = Vec::new();
                        let mut readable = true;
                        for blob in record.blobs() {
                            if blob.kind() != 2 {
                                readable = false;
                                break;
                            }
                            match file.read_time_lapse_blob(&database, blob, file.limits()) {
                                Ok(bytes) if payload.len().saturating_add(bytes.len()) <= 128 * 1024 * 1024 => {
                                    payload.extend_from_slice(&bytes);
                                }
                                _ => {
                                    readable = false;
                                    break;
                                }
                            }
                        }
                        let frames = if readable { extract_png_frames(&payload) } else { None };
                        match frames {
                            Some(frames) if !frames.is_empty() => time_lapse_frames.extend(frames),
                            _ => unsupported_time_lapse_records = unsupported_time_lapse_records.saturating_add(1),
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(_) => unsupported_time_lapse_records = 1,
        }
        let unsupported_animation_tracks = match file.read_animation(&database, file.limits()) {
            Ok(Some(animation)) => animation
                .tracks()
                .len()
                .saturating_add(animation.animation_tracks().len())
                .try_into()
                .unwrap_or(u32::MAX),
            Ok(None) => 0,
            Err(_) => 1,
        };
        let mut vector_strokes = std::collections::BTreeMap::new();
        let mut vector_source_layers = 0_u32;
        for (source_id, target_id) in &source_to_target {
            let sources = database.vector_data_sources(*source_id, file.limits())?;
            let mut converted = Vec::new();
            let mut raw_sources = Vec::new();
            let mut unsupported = false;
            for source in sources {
                let vector = file.read_vector(&database, &source, file.limits())?;
                raw_sources.push(vector.raw().to_vec());
                for stroke in vector.strokes() {
                    let Some(style) = stroke.style() else {
                        unsupported = true;
                        continue;
                    };
                    if !stroke.is_semantically_supported() || style.kind() != VectorStrokeKind::Normal {
                        unsupported = true;
                        continue;
                    }
                    let Some(width) = style.brush_diameter().filter(|width| width.is_finite() && *width > 0.0) else {
                        unsupported = true;
                        continue;
                    };
                    let mut points = Vec::new();
                    for point in stroke.points() {
                        let Some(position) = point.position() else {
                            continue;
                        };
                        points.push(ClipVectorPoint {
                            position: [position.x() as f32, position.y() as f32],
                            width_factor: point.width_factor().unwrap_or(1.0),
                            opacity_factor: point.opacity_factor().unwrap_or(1.0),
                        });
                    }
                    if points.len() < 2 {
                        unsupported = true;
                        continue;
                    }
                    let color = style.main_color().rgb_8bit();
                    converted.push(ClipVectorStroke {
                        points,
                        color: [color[0], color[1], color[2]],
                        opacity: style.opacity() as f32,
                        width: width as f32,
                        closed: stroke.is_closed(),
                    });
                }
            }
            if !raw_sources.is_empty() {
                let mut data = b"HCVD".to_vec();
                data.extend_from_slice(&1_u32.to_le_bytes());
                data.extend_from_slice(&(raw_sources.len() as u32).to_le_bytes());
                for source in raw_sources {
                    data.extend_from_slice(&(source.len() as u64).to_le_bytes());
                    data.extend_from_slice(&source);
                }
                if !document.set_plugin_layer(*target_id, "com.hanacreate.clip-vector".to_owned(), data) {
                    return Err(ClipError::Invalid("vector layer attachment"));
                }
                vector_source_layers = vector_source_layers.saturating_add(1);
            }
            if !converted.is_empty() {
                if !pixel_sources.iter().any(|(id, _)| id == target_id) {
                    pixel_sources.push((*target_id, PixelSource::Layer(*source_id)));
                }
                vector_strokes.insert(*target_id, converted);
            }
            if unsupported {
                unsupported_layers = unsupported_layers.saturating_add(1);
            }
        }
        let canvas_bytes = u64::from(width) * u64::from(height) * 4;
        let raster_budget = layer_budget().saturating_sub(canvas_bytes.saturating_mul(4));
        let mut skipped_raster_layers = std::collections::BTreeSet::new();
        for (target_id, source) in &pixel_sources {
            let raster_source = match source {
                PixelSource::Layer(source_id) => database.layer_raster_source(*source_id)?,
                PixelSource::Mask(source_id) => database.layer_mask_raster_source(*source_id)?,
            };
            if let Some(raster_source) = raster_source {
                let attributes = raster_source.attributes();
                let bytes = u64::from(attributes.bitmap_width())
                    .checked_mul(u64::from(attributes.bitmap_height()))
                    .and_then(|pixels| pixels.checked_mul(4))
                    .ok_or(ClipError::TooLarge)?;
                if bytes > raster_budget {
                    return Err(ClipError::TooLarge);
                }
                let width = canvas.width().round() as u32;
                let height = canvas.height().round() as u32;
                let raster_width = attributes.bitmap_width();
                let raster_height = attributes.bitmap_height();
                if raster_width < width
                    || raster_height < height
                    || raster_width - width > 255
                    || raster_height - height > 255
                {
                    let source_id = match source {
                        PixelSource::Layer(source_id) | PixelSource::Mask(source_id) => *source_id,
                    };
                    if source_document.layer(source_id).is_some_and(|layer| !layer.is_visible()) {
                        if !vector_strokes.contains_key(target_id) {
                            unsupported_layers = unsupported_layers.saturating_add(1);
                        }
                        skipped_raster_layers.insert(*target_id);
                    } else {
                        return Err(ClipError::Unsupported("raster bounds"));
                    }
                }
            }
        }
        let preview = database
            .canvas_preview(canvas_id, file.limits())?
            .filter(clipfile::CanvasPreview::is_png)
            .map(|preview| preview.data().to_vec());
        Ok(Self {
            file,
            database,
            document,
            canvas_id,
            pixel_sources,
            vector_strokes,
            vector_source_layers,
            skipped_raster_layers,
            time_lapse_frames,
            unsupported_time_lapse_records,
            unsupported_animation_tracks,
            preview,
            approximated_blends,
            source_blend_modes,
            rasterized_layers,
            unsupported_layers,
        })
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn layers_with_pixels(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.pixel_sources.iter().map(|(id, _)| *id)
    }

    pub fn pixels(&mut self, id: LayerId) -> Result<Option<Vec<u8>>, ClipError> {
        if self.skipped_raster_layers.contains(&id) {
            return Ok(None);
        }
        let Some((_, source)) = self.pixel_sources.iter().find(|(layer_id, _)| *layer_id == id) else {
            return Ok(None);
        };
        let source = *source;
        let raster_source = match source {
            PixelSource::Layer(source_id) => self.database.layer_raster_source(source_id)?,
            PixelSource::Mask(source_id) => self.database.layer_mask_raster_source(source_id)?,
        };
        let Some(raster_source) = raster_source else {
            return Ok(None);
        };
        let raster = self.file.decode_raster(&self.database, &raster_source)?;
        if raster.data_state() != RasterDataState::Present {
            return Ok(None);
        }
        let width = self.document.width();
        let height = self.document.height();
        let source_width = raster.width();
        let source_height = raster.height();
        if source_width < width || source_height < height || source_width - width > 255 || source_height - height > 255
        {
            return Err(ClipError::Unsupported("raster bounds"));
        }
        let format = raster.format();
        let mut input = raster.into_pixels();
        if format == PixelFormat::Rgba8 {
            let input_stride = source_width as usize * 4;
            let output_stride = width as usize * 4;
            if input_stride != output_stride {
                for y in 0..height as usize {
                    input.copy_within(y * input_stride..y * input_stride + output_stride, y * output_stride);
                }
                input.truncate(output_stride * height as usize);
            }
            if matches!(source, PixelSource::Mask(_)) {
                for pixel in input.chunks_exact_mut(4) {
                    let gray =
                        ((u16::from(pixel[0]) * 299 + u16::from(pixel[1]) * 587 + u16::from(pixel[2]) * 114 + 500)
                            / 1000) as u8;
                    pixel.copy_from_slice(&[gray, gray, gray, 255]);
                }
            }
            return Ok(Some(input));
        }
        let source_bpp = match format {
            PixelFormat::Gray8 => 1,
            PixelFormat::GrayAlpha8 => 2,
            _ => return Err(ClipError::Unsupported("raster pixel format")),
        };
        let mut output = vec![0_u8; width as usize * height as usize * 4];
        let input_stride = source_width as usize * source_bpp;
        let output_stride = width as usize * 4;
        for y in 0..height as usize {
            let source_row = &input[y * input_stride..y * input_stride + width as usize * source_bpp];
            let output_row = &mut output[y * output_stride..(y + 1) * output_stride];
            match format {
                PixelFormat::Gray8 => {
                    for (gray, rgba) in source_row.iter().zip(output_row.chunks_exact_mut(4)) {
                        rgba.copy_from_slice(&[*gray, *gray, *gray, 255]);
                    }
                }
                PixelFormat::GrayAlpha8 => {
                    for (pixel, rgba) in source_row.chunks_exact(2).zip(output_row.chunks_exact_mut(4)) {
                        rgba.copy_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
                    }
                }
                _ => return Err(ClipError::Unsupported("raster pixel format")),
            }
        }
        Ok(Some(output))
    }

    pub fn vector_strokes(&self, id: LayerId) -> Option<&[ClipVectorStroke]> {
        self.vector_strokes.get(&id).map(Vec::as_slice)
    }

    pub fn vector_source_layer_count(&self) -> u32 {
        self.vector_source_layers
    }

    pub fn preview(&self) -> Option<&[u8]> {
        self.preview.as_deref()
    }

    pub fn canvas_id(&self) -> i64 {
        self.canvas_id
    }

    pub fn approximated_blends(&self) -> &[(i64, i64)] {
        &self.approximated_blends
    }

    pub fn source_blend_modes(&self) -> &[(LayerId, i64)] {
        &self.source_blend_modes
    }

    pub fn rasterized_layer_count(&self) -> u32 {
        self.rasterized_layers
    }

    pub fn unsupported_layer_count(&self) -> u32 {
        self.unsupported_layers
    }

    pub fn time_lapse_frames(&self) -> &[Vec<u8>] {
        &self.time_lapse_frames
    }

    pub fn unsupported_time_lapse_records(&self) -> u32 {
        self.unsupported_time_lapse_records
    }

    pub fn unsupported_animation_tracks(&self) -> u32 {
        self.unsupported_animation_tracks
    }
}

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const MAX_TIME_LAPSE_FRAMES: usize = 240;

fn extract_png_frames(payload: &[u8]) -> Option<Vec<Vec<u8>>> {
    let mut frames = Vec::new();
    let mut search_from = 0_usize;
    while search_from < payload.len() {
        let Some(relative) =
            payload.get(search_from..)?.windows(PNG_SIGNATURE.len()).position(|window| window == PNG_SIGNATURE)
        else {
            break;
        };
        let start = search_from.checked_add(relative)?;
        let end = png_frame_end(payload, start)?;
        frames.push(payload.get(start..end)?.to_vec());
        if frames.len() > MAX_TIME_LAPSE_FRAMES {
            return None;
        }
        search_from = end;
    }
    Some(frames)
}

fn png_frame_end(payload: &[u8], start: usize) -> Option<usize> {
    if payload.get(start..start.checked_add(PNG_SIGNATURE.len())?)? != PNG_SIGNATURE {
        return None;
    }
    let mut cursor = start.checked_add(PNG_SIGNATURE.len())?;
    loop {
        let header_end = cursor.checked_add(8)?;
        let header = payload.get(cursor..header_end)?;
        let length = usize::try_from(u32::from_be_bytes(header.get(..4)?.try_into().ok()?)).ok()?;
        let kind = header.get(4..8)?;
        let end = header_end.checked_add(length)?.checked_add(4)?;
        if end > payload.len() {
            return None;
        }
        if kind == b"IEND" {
            return Some(end);
        }
        cursor = end;
    }
}

fn append_children(
    tree: &clipfile::LayerTree,
    source_document: &clipfile::Document,
    source_parent: i64,
    target_parent: Option<LayerId>,
    layers: &mut Vec<Layer>,
    pixel_sources: &mut Vec<(LayerId, PixelSource)>,
    source_to_target: &mut std::collections::BTreeMap<i64, LayerId>,
    approximated_blends: &mut Vec<(i64, i64)>,
    rasterized_layers: &mut u32,
    unsupported_layers: &mut u32,
    next_id: &mut u64,
) -> Result<(), ClipError> {
    for source_id in tree.children_of(source_parent).ok_or(ClipError::Invalid("layer tree"))? {
        let source = source_document.layer(*source_id).ok_or(ClipError::Invalid("layer"))?;
        let id = LayerId(*next_id);
        *next_id = next_id.checked_add(1).ok_or(ClipError::Invalid("layer count"))?;
        source_to_target.insert(*source_id, id);
        let kind = if source.is_folder() { LayerKind::Group } else { LayerKind::Paint };
        let mut layer = Layer::plain(
            id,
            kind,
            source.name().map(str::to_owned).unwrap_or_else(|| "Layer".to_owned()),
            target_parent,
        );
        layer.visible = source.is_visible();
        layer.opacity = source.opacity_fraction() as f32;
        let raw_blend = source.blend_mode().raw();
        layer.blend = blend_mode(raw_blend);
        if blend_mode_is_approximated(raw_blend) {
            approximated_blends.push((*source_id, raw_blend));
        }
        layer.clipped = source.is_clipped();
        layer.locked = source.lock_flags() != 0;
        layer.collapsed = source.is_folder_closed();
        if !source.is_folder() && !source.kind().is_pixel() && source.kind().raw() != 1584 {
            if source.render_mipmap_id().is_some() {
                *rasterized_layers = rasterized_layers.saturating_add(1);
            } else {
                *unsupported_layers = unsupported_layers.saturating_add(1);
            }
        }
        if source.is_folder() {
            append_children(
                tree,
                source_document,
                *source_id,
                Some(id),
                layers,
                pixel_sources,
                source_to_target,
                approximated_blends,
                rasterized_layers,
                unsupported_layers,
                next_id,
            )?;
            layers.push(layer);
        } else {
            layers.push(layer);
            if source.render_mipmap_id().is_some() {
                pixel_sources.push((id, PixelSource::Layer(*source_id)));
            }
            if source.mask_mipmap_id().is_some() {
                let mask_id = LayerId(*next_id);
                *next_id = next_id.checked_add(1).ok_or(ClipError::Invalid("layer count"))?;
                let mut mask = Layer::plain(mask_id, LayerKind::Mask, "Mask".to_owned(), Some(id));
                mask.visible = true;
                layers.push(mask);
                pixel_sources.push((mask_id, PixelSource::Mask(*source_id)));
            }
            append_children(
                tree,
                source_document,
                *source_id,
                Some(id),
                layers,
                pixel_sources,
                source_to_target,
                approximated_blends,
                rasterized_layers,
                unsupported_layers,
                next_id,
            )?;
        }
    }
    Ok(())
}

fn blend_mode_is_approximated(raw: i64) -> bool {
    matches!(raw, 10 | 12 | 26 | 30) || !matches!(raw, 0..=25 | 36)
}

fn blend_mode(raw: i64) -> BlendMode {
    match raw {
        0 => BlendMode::Normal,
        1 => BlendMode::Darken,
        2 => BlendMode::Multiply,
        3 => BlendMode::ColorBurn,
        4 => BlendMode::LinearBurn,
        5 => BlendMode::Subtract,
        6 => BlendMode::DarkerColor,
        7 => BlendMode::Lighten,
        8 => BlendMode::Screen,
        9 | 10 => BlendMode::ColorDodge,
        11 | 12 => BlendMode::Add,
        13 => BlendMode::LighterColor,
        14 => BlendMode::Overlay,
        15 => BlendMode::SoftLight,
        16 => BlendMode::HardLight,
        17 => BlendMode::VividLight,
        18 => BlendMode::LinearLight,
        19 => BlendMode::PinLight,
        20 => BlendMode::HardMix,
        21 => BlendMode::Difference,
        22 => BlendMode::Exclusion,
        23 => BlendMode::Hue,
        24 => BlendMode::Saturation,
        25 => BlendMode::Color,
        26 => BlendMode::Luminosity,
        36 => BlendMode::Divide,
        _ => BlendMode::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_complete_png_frames_from_a_v2_payload() {
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend_from_slice(&13_u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0; 13]);
        png.extend_from_slice(&[0; 4]);
        png.extend_from_slice(&0_u32.to_be_bytes());
        png.extend_from_slice(b"IEND");
        png.extend_from_slice(&[0; 4]);
        let mut payload = b"t2".to_vec();
        payload.extend_from_slice(&png);
        payload.extend_from_slice(b"trailing metadata");
        payload.extend_from_slice(&png);
        assert_eq!(extract_png_frames(&payload), Some(vec![png.clone(), png]));
    }

    #[test]
    fn rejects_truncated_png_frames() {
        let mut payload = PNG_SIGNATURE.to_vec();
        payload.extend_from_slice(&32_u32.to_be_bytes());
        payload.extend_from_slice(b"IHDR");
        assert_eq!(extract_png_frames(&payload), None);
    }

    #[test]
    fn marks_unrepresentable_clip_blends_as_approximations() {
        assert!(!blend_mode_is_approximated(9));
        assert!(blend_mode_is_approximated(10));
        assert!(!blend_mode_is_approximated(11));
        assert!(blend_mode_is_approximated(12));
        assert!(blend_mode_is_approximated(26));
        assert!(blend_mode_is_approximated(30));
        assert!(!blend_mode_is_approximated(36));
        assert!(blend_mode_is_approximated(99));
    }

    #[test]
    #[ignore = "requires HANACREATE_CLIP_TEST_FILE"]
    fn reports_configured_clip_time_lapse_records() {
        let path = std::env::var("HANACREATE_CLIP_TEST_FILE").unwrap();
        let mut artwork = ClipArtwork::open(std::path::Path::new(&path)).unwrap();
        eprintln!(
            "imported frames={} unsupported records={}",
            artwork.time_lapse_frames().len(),
            artwork.unsupported_time_lapse_records()
        );
        let time_lapse = artwork.database.time_lapse(artwork.file.limits()).unwrap();
        for manager in time_lapse.iter().flat_map(clipfile::TimeLapse::managers) {
            for record in manager.records() {
                eprintln!(
                    "encoder={} sequence={} blobs={:?}",
                    record.encoder_name(),
                    record.encoder_sequence(),
                    record.blobs().iter().map(|blob| (blob.kind(), blob.decoded_size())).collect::<Vec<_>>()
                );
                for blob in record.blobs() {
                    let bytes =
                        artwork.file.read_time_lapse_blob(&artwork.database, blob, artwork.file.limits()).unwrap();
                    let signatures = [b"GMIK".as_slice(), b"GMID", b"RIFF", b"WEBP", b"PNG", b"ftyp", b"JFIF"];
                    let counts = signatures
                        .map(|signature| bytes.windows(signature.len()).filter(|window| *window == signature).count());
                    eprintln!(
                        "blob={} size={} prefix={:02x?} signatures={counts:?}",
                        blob.id(),
                        bytes.len(),
                        &bytes[..bytes.len().min(32)]
                    );
                }
            }
        }
    }

    #[test]
    #[ignore = "requires HANACREATE_CLIP_TEST_FILE"]
    fn reports_configured_clip_layer_blend_modes() {
        let path = std::env::var("HANACREATE_CLIP_TEST_FILE").unwrap();
        let artwork = ClipArtwork::open(std::path::Path::new(&path)).unwrap();
        eprintln!("hybrid vector source layers={}", artwork.vector_source_layer_count());
        for layer in artwork.document().layers() {
            let raw = artwork.source_blend_modes().iter().find(|(id, _)| *id == layer.id).map(|(_, raw)| *raw);
            eprintln!("{} | {:?} | {:?}", layer.name, raw, layer.blend);
        }
        eprintln!("approximated: {:?}", artwork.approximated_blends());
    }

    #[test]
    #[ignore = "requires HANACREATE_CLIP_VECTOR_TEST_FILE"]
    fn reads_configured_clip_vector_strokes() {
        let path = std::env::var("HANACREATE_CLIP_VECTOR_TEST_FILE").unwrap();
        let artwork = ClipArtwork::open(std::path::Path::new(&path)).unwrap();
        let strokes = artwork
            .document()
            .layers()
            .iter()
            .filter_map(|layer| artwork.vector_strokes(layer.id))
            .map(<[ClipVectorStroke]>::len)
            .sum::<usize>();
        assert!(strokes > 0);
    }

    #[test]
    #[ignore = "requires HANACREATE_CLIP_TEST_FILE"]
    fn imports_configured_clip_time_lapse_frames() {
        let path = std::env::var("HANACREATE_CLIP_TEST_FILE").unwrap();
        let artwork = ClipArtwork::open(std::path::Path::new(&path)).unwrap();
        assert_eq!(artwork.time_lapse_frames().len(), 12);
        assert_eq!(artwork.unsupported_time_lapse_records(), 0);
    }

    #[test]
    #[ignore = "requires HANACREATE_CLIP_TEST_FILE"]
    fn imports_configured_clip_corpus() {
        let path = std::env::var("HANACREATE_CLIP_TEST_FILE").unwrap();
        let mut artwork = ClipArtwork::open(std::path::Path::new(&path)).unwrap();
        assert!(artwork.document().width() > 0);
        assert!(artwork.document().height() > 0);
        assert!(!artwork.document().layers().is_empty());
        for id in artwork.layers_with_pixels().collect::<Vec<_>>() {
            if let Some(pixels) = artwork.pixels(id).unwrap() {
                assert_eq!(
                    pixels.len(),
                    artwork.document().width() as usize * artwork.document().height() as usize * 4
                );
            }
        }
    }
}
