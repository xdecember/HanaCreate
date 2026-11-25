//! PSDs brought in as artworks: written a layer at a time into files the artwork opens, the file
//! only read; turned away whole past the layer cap, leaving nothing; never half made in the
//! gallery; and an artwork whose opening ended the app told when picked again, still in the
//! gallery, and opened again without a crash.

use std::fs;
use std::path::{Path, PathBuf};

use procraft_engine::document::{BlendMode, LayerKind};
use procraft_engine::formats::{PsdKind, PsdLayer, psd};

use crate::{
    ArtworkStore, CanvasSession, ClipOutcome, GalleryEntry, PsdOutcome, import_clip, import_psd, opening_ended_app,
};

/// An empty folder of the test's own.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("procraft-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn layer<'a>(name: &'a str, kind: PsdKind<'a>) -> PsdLayer<'a> {
    PsdLayer { name, kind, opacity: 1.0, visible: true, blend: BlendMode::Normal, clipped: false }
}

/// A small PSD: a group holding a masked layer of soft colour, over a base; and its bytes.
fn small_psd(w: u32, h: u32) -> Vec<u8> {
    let n = (w * h) as usize;
    let base: Vec<u8> = (0..n).flat_map(|i| [(i % 200) as u8, 90, 30, 255]).collect();
    let ink: Vec<u8> = (0..n).flat_map(|i| if i % 3 == 0 { [10, 20, (i % 250) as u8, 180] } else { [0; 4] }).collect();
    let mask: Vec<u8> = (0..n).map(|i| (i * 7 % 256) as u8).collect();
    let layers = [
        layer("Folder", PsdKind::GroupStart { open: true }),
        PsdLayer { blend: BlendMode::Multiply, ..layer("Ink", PsdKind::Pixels { rgba: &ink, mask: Some(&mask) }) },
        layer("", PsdKind::GroupEnd),
        layer("Base", PsdKind::Pixels { rgba: &base, mask: None }),
    ];
    psd(w, h, 300.0, &[], &layers, &base, None)
}

/// The artworks the gallery in `store` lists, by name.
fn listed(store: &ArtworkStore) -> Vec<String> {
    store
        .list(None)
        .into_iter()
        .filter_map(|e| match e {
            GalleryEntry::Artwork { artwork } => Some(artwork.name),
            GalleryEntry::Stack { .. } => None,
        })
        .collect()
}

#[test]
fn a_psd_comes_in_a_layer_at_a_time_as_the_files_the_artwork_opens_and_is_only_read() {
    let root = folder("psd-import");
    let (w, h) = (40, 30);
    let bytes = small_psd(w, h);
    let path = root.join("in.psd");
    fs::write(&path, &bytes).unwrap();
    let store = ArtworkStore::open(root.join("gallery").to_string_lossy().into_owned());
    let dir = store.new_folder(None);
    let outcome = import_psd(path.to_string_lossy().into_owned(), dir.clone(), "Imported".to_owned());
    assert_eq!(outcome, PsdOutcome::Imported);
    // the file only read
    assert_eq!(fs::read(&path).unwrap(), bytes);
    // in the gallery whole, its import's note gone
    assert_eq!(listed(&store), ["Imported"]);
    assert!(!Path::new(&dir).join("import.run").exists());
    assert!(Path::new(&dir).join("thumbnail.png").exists());
    // it opens as the file has it: each layer's file the pixels the PSD reads as
    let read = procraft_engine::psd::read(&bytes).unwrap();
    let s = CanvasSession::open(dir.clone()).unwrap();
    let st = s.shared.lock();
    let names: Vec<(&str, LayerKind)> = st.doc.layers().iter().map(|l| (l.name.as_str(), l.kind)).collect();
    assert_eq!(
        names,
        [
            ("Base", LayerKind::Paint),
            ("Ink", LayerKind::Paint),
            ("Mask", LayerKind::Mask),
            ("Folder", LayerKind::Group)
        ]
    );
    assert_eq!(st.doc.layers()[1].blend, BlendMode::Multiply);
    assert_eq!(st.pending_pixels.len(), 3);
    for (id, opened) in &st.pending_pixels {
        let want = read.pixels.iter().find(|(i, _)| i == id).map(|(_, p)| p.clone()).unwrap();
        assert_eq!(opened.clone().pixels(w, h).unwrap(), want, "layer {id:?}");
    }
    drop(st);
    drop(s);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_psd_of_more_layers_than_any_canvas_holds_is_turned_away_whole_leaving_nothing() {
    // 1001 layers: past the most a canvas of any size holds (1000) on any device; such a file is
    // turned away whole (neither the layers up to the cap nor the rest merged come in)
    let root = folder("psd-too-many");
    let (w, h) = (4, 4);
    let mut dot = vec![0u8; (w * h * 4) as usize];
    dot[..4].copy_from_slice(&[200, 0, 0, 255]);
    let layers: Vec<PsdLayer<'_>> =
        (0..1001).map(|_| layer("Dot", PsdKind::Pixels { rgba: &dot, mask: None })).collect();
    let bytes = psd(w, h, 72.0, &[], &layers, &dot, None);
    let path = root.join("many.psd");
    fs::write(&path, &bytes).unwrap();
    let store = ArtworkStore::open(root.join("gallery").to_string_lossy().into_owned());
    let dir = store.new_folder(None);
    let outcome = import_psd(path.to_string_lossy().into_owned(), dir.clone(), "Many".to_owned());
    assert_eq!(outcome, PsdOutcome::TooManyLayers { most: 1000 });
    // nothing written: no artwork, no layer, nothing in the gallery
    let written: Vec<_> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
    assert!(written.is_empty(), "{written:?}");
    assert!(listed(&store).is_empty());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_import_the_app_stopped_during_is_never_an_artwork_and_goes_when_the_gallery_next_opens() {
    let root = folder("psd-cut-short");
    let gallery = root.join("gallery");
    let store = ArtworkStore::open(gallery.to_string_lossy().into_owned());
    let path = root.join("in.psd");
    fs::write(&path, small_psd(20, 10)).unwrap();
    // one whole, and one as an import stopped just before its end leaves it: whole but for its note
    let whole = store.new_folder(None);
    assert_eq!(import_psd(path.to_string_lossy().into_owned(), whole, "Whole".to_owned()), PsdOutcome::Imported);
    let cut = store.new_folder(None);
    assert_eq!(import_psd(path.to_string_lossy().into_owned(), cut.clone(), "Cut".to_owned()), PsdOutcome::Imported);
    fs::write(Path::new(&cut).join("import.run"), "1").unwrap();
    assert_eq!(listed(&store), ["Whole"]);
    // an import under way in this run is left alone; one a run that has ended left is taken away
    fs::write(Path::new(&cut).join("import.run"), std::process::id().to_string()).unwrap();
    drop(ArtworkStore::open(gallery.to_string_lossy().into_owned()));
    assert!(Path::new(&cut).exists());
    fs::write(Path::new(&cut).join("import.run"), "1").unwrap();
    let again = ArtworkStore::open(gallery.to_string_lossy().into_owned());
    assert!(!Path::new(&cut).exists());
    assert_eq!(listed(&again), ["Whole"]);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_artwork_whose_opening_ended_the_app_is_told_when_picked_and_opens_again_without_a_crash() {
    let root = folder("open-ended-app");
    let marks = root.join("opening.txt");
    crate::open_marks::use_file_on_this_thread(Some(marks.clone()));
    let store = ArtworkStore::open(root.join("gallery").to_string_lossy().into_owned());
    let path = root.join("in.psd");
    fs::write(&path, small_psd(24, 16)).unwrap();
    let dir = store.new_folder(None);
    assert_eq!(import_psd(path.to_string_lossy().into_owned(), dir.clone(), "Art".to_owned()), PsdOutcome::Imported);
    // being opened: noted, but not told while this run goes on
    let s = CanvasSession::open(dir.clone()).unwrap();
    assert!(!opening_ended_app(dir.clone()));
    // the app ends while it opens (stopped for memory): its note stays, from a run that has ended
    std::mem::forget(s);
    let noted = fs::read_to_string(&marks).unwrap();
    fs::write(&marks, noted.replace(&format!("{} ", std::process::id()), "1 ")).unwrap();
    // picked again: told rather than opened, and still in the gallery
    assert!(opening_ended_app(dir.clone()));
    assert_eq!(listed(&store), ["Art"]);
    // opened anyway: it opens, and is no longer told once it shows or is let go
    let s = CanvasSession::open(dir.clone()).unwrap();
    assert!(!opening_ended_app(dir.clone()));
    drop(s);
    assert_eq!(fs::read_to_string(&marks).unwrap(), "");
    // shown: its first frame drawn takes its note away (on a GPU, where there is one)
    match procraft_gpu::Gpu::headless() {
        Err(e) => eprintln!("the frame's part skipped: {e}"),
        Ok(gpu) => {
            use procraft_gpu::wgpu;
            let s = CanvasSession::open(dir.clone()).unwrap();
            assert!(
                fs::read_to_string(&marks).unwrap().contains(Path::new(&dir).file_name().unwrap().to_str().unwrap())
            );
            let format = wgpu::TextureFormat::Rgba8Unorm;
            let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("window"),
                size: wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = target.create_view(&Default::default());
            {
                let mut st = s.shared.lock();
                st.window_size = (64, 64);
                st.view = Some(procraft_engine::view::View::fit((24, 16), (64, 64)));
                st.gpu = Some(gpu.clone());
                st.ensure_canvas();
            }
            let started = std::time::Instant::now();
            for _ in 0..60 {
                let mut st = s.shared.lock();
                st.draw_frame(&view, format, (64, 64), procraft_engine::profile::Profile::Srgb, started);
                gpu.flush();
                if st.open_mark.is_none() {
                    break;
                }
            }
            assert!(s.shared.lock().open_mark.is_none(), "no frame of it was drawn");
            // (its note is written away off the frame's thread)
            let gone = (0..200).any(|_| {
                std::thread::sleep(std::time::Duration::from_millis(10));
                fs::read_to_string(&marks).is_ok_and(|m| m.is_empty())
            });
            assert!(gone, "still noted once shown");
            assert!(!opening_ended_app(dir.clone()));
            drop(s);
        }
    }
    // and it can be deleted from the gallery
    store.delete(dir);
    assert!(listed(&store).is_empty());
    crate::open_marks::use_file_on_this_thread(None);
    let _ = fs::remove_dir_all(&root);
}

#[test]
#[ignore = "requires HANACREATE_CLIP_VECTOR_TEST_FILE"]
fn imports_clip_vector_layers_as_visible_raster_content() {
    let path = std::env::var_os("HANACREATE_CLIP_VECTOR_TEST_FILE").unwrap();
    let root = folder("clip-vector-import");
    let store = ArtworkStore::open(root.join("gallery").to_string_lossy().into_owned());
    let dir = store.new_folder(None);
    let outcome = import_clip(path.to_string_lossy().into_owned(), dir.clone(), "CLIP vectors".to_owned());
    assert!(matches!(outcome, ClipOutcome::Imported { rasterized_layers: 3, .. }));
    let layers = std::fs::read_dir(Path::new(&dir).join("layers")).unwrap();
    assert_eq!(
        layers.filter_map(Result::ok).filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png")).count(),
        4
    );
}

#[test]
#[ignore = "requires HANACREATE_CLIP_TEST_FILE"]
fn imports_configured_clip_file_into_native_artwork() {
    let path = std::env::var_os("HANACREATE_CLIP_TEST_FILE").unwrap();
    let root = folder("clip-import");
    let store = ArtworkStore::open(root.join("gallery").to_string_lossy().into_owned());
    let dir = store.new_folder(None);
    let outcome = import_clip(path.to_string_lossy().into_owned(), dir.clone(), "CLIP import".to_owned());
    let ClipOutcome::Imported { preserved_vector_layers, source_clip_preserved, .. } = outcome else {
        panic!("CLIP import outcome: {outcome:?}");
    };
    assert!(preserved_vector_layers > 0, "no hybrid vector layers were preserved");
    assert!(source_clip_preserved, "the source CLIP archive was not retained");
    assert_eq!(listed(&store), ["CLIP import"]);
    assert!(Path::new(&dir).join("artwork.json").exists());
    assert!(Path::new(&dir).join("thumbnail.png").exists());
    let session = CanvasSession::open(dir.clone()).unwrap();
    let state = session.shared.lock();
    assert!(state.doc.width() > 0);
    assert!(state.doc.height() > 0);
    assert!(!state.pending_pixels.is_empty());
    let vector = state
        .doc
        .layers()
        .iter()
        .find(|layer| layer.plugin.as_ref().is_some_and(|plugin| plugin.kind == "com.hanacreate.clip-vector"))
        .unwrap();
    let data = vector.plugin.as_ref().and_then(|plugin| plugin.data.as_deref()).unwrap();
    assert_eq!(&data[..4], b"HCVD");
    assert!(Path::new(&dir).join("layers").join(format!("{}.layerdata", vector.id.0)).exists());
    assert!(Path::new(&dir).join("source.clip").exists());
    assert_eq!(crate::timelapse::TimeLapse::new(dir.clone()).count(), 12);
    let vector_id = vector.id.0;
    drop(state);
    assert_eq!(session.plugin_layer_kind(vector_id), None);
    let info = session.layers().into_iter().find(|layer| layer.id == vector_id).unwrap();
    assert!(!info.locked);
}

#[test]
#[ignore = "requires HANACREATE_CLIP_TEST_FILE"]
fn retains_v1_clip_archive_when_its_time_lapse_is_not_decoded() {
    let path = std::env::var_os("HANACREATE_CLIP_TEST_FILE").unwrap();
    let root = folder("clip-v1-archive");
    let store = ArtworkStore::open(root.join("gallery").to_string_lossy().into_owned());
    let dir = store.new_folder(None);
    let outcome = import_clip(path.to_string_lossy().into_owned(), dir.clone(), "CLIP V1".to_owned());
    let ClipOutcome::Imported {
        imported_time_lapse_frames, unsupported_time_lapse_records, source_clip_preserved, ..
    } = outcome
    else {
        panic!("CLIP import outcome: {outcome:?}");
    };
    assert_eq!(imported_time_lapse_frames, 0);
    assert_eq!(unsupported_time_lapse_records, 1);
    assert!(source_clip_preserved);
    let source = Path::new(&dir).join("source.clip");
    assert_eq!(fs::metadata(source).unwrap().len(), fs::metadata(path).unwrap().len());
    assert_eq!(crate::timelapse::TimeLapse::new(dir).count(), 0);
}
