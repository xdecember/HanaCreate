# HanaCreate architecture and implementation plan

Status: baseline inspection and implementation validation, 2026-11-25.

## Product boundary

HanaCreate is the product. ProCraft is its upstream compatibility baseline, not the product identity. Preserve the existing Rust drawing engine and cross-platform document behavior while moving product-facing identity, persistence, rendering capabilities, and desktop presentation behind HanaCreate-owned boundaries. Keep upstream import/export compatibility explicit; do not silently fork or rename every internal crate identifier before there is a tested migration plan.

## Verified baseline

- Workspace: Rust 2024; crates/engine, crates/gpu, crates/core, desktop (egui), Android (Kotlin/Compose), iOS (SwiftUI).
- GPU renderer: wgpu 30 and WGSL. The engine owns document/layer domain state; GPU owns raster storage/compositing.
- Layer blend support already exists for 27 modes in engine BlendMode::ALL and blend.wgsl: Multiply, Darken, Shade, Color Burn, Linear Burn, Darker Color, Normal, Lighten, Screen, Color Dodge, Add, Lighter Color, Overlay, Soft Light, Hard Light, Vivid Light, Linear Light, Pin Light, Hard Mix, Difference, Exclusion, Subtract, Divide, Hue, Saturation, Color, Luminosity. The layer panel obtains this list from procraft_core::blend_modes().
- Solo is a layer visibility/isolation operation, not a pixel blend equation. Clip Studio Paint Brightness is a component blend mode distinct from Luminosity; the importer currently approximates it as Luminosity. Glow Dodge and Add (glow) are not separate native BlendMode variants yet and need verified compositing equations and reference tests.
- A bundled vector-layers WASM plugin already exists and declares a vector layer kind and SVG export. This proves an extension exists, not that vector strokes have first-class native ownership, brush-tool parity, persistence guarantees, or seamless raster effects.
- Native document code currently reads a Procreate-compatible ZIP package (Document.archive, 256 px LZ4 tiles, preview and time-lapse segments). This is an interoperability path, not HanaCreate's own format.
- Existing GPU layer tiles and memory-trimming paths exist. Large-canvas performance work must measure allocation, residency, upload/readback, compositing and frame pacing rather than assume the renderer has no memory policy.
- HVS core currently exposes CRC32C and SHA-256 primitives and a source-backed HVS-Archive reader/verifier. The documented HVS v2 container has no writer in the inspected HVS workspace; do not make production saves depend on a nonexistent writer or conflate HVS v2 with HVFS.
- Zig 0.17.1 and Rust 1.98.0 are installed on the development machine.
- The current CLIP importer uses `clipfile` 1.2.0. It reads canvas metadata, layer hierarchy, raster mipmaps, masks, blend IDs, vector records, previews, time-lapse records and animation metadata. CLIP vector layers retain their original raw vector records in native layer-data sidecars and keep a raster projection for immediate display and paint compatibility. The projection now uses per-point width and opacity factors rather than averaging them across a stroke. The original source `.clip` is also retained as `source.clip` when the source file is at most 512 MiB. The HanaCreate vector-path editor is not yet connected to these records. For the supplied 13.5 MiB `Illustration2clip`, the complete source archive is retained, including referenced brush records; velocity/stabilization settings are preserved only as source data and are not yet decoded or replayed by HanaCreate.
- `Illustration1clip` is 2280 × 3450 at 300 DPI with 47 layers and 45 pixel sources. All 47 layers were inspected individually and have raw blend mode 0 (Normal). It has one V1 time-lapse record spread over 11 blobs (565,314,008 decoded bytes), which remains unsupported. `Illustration2clip` is 4961 × 7016 at 600 DPI with five layers, 723 vector strokes and one V2 time-lapse record (13,231,811 decoded bytes, 12 PNG snapshots). Its V2 snapshots are imported into HanaCreate time-lapse JPEG frames; the original timing is not retained. Its original source file is preserved beside the artwork, and the vector layer's raw source records are stored in its native layer-data sidecar. Both samples have only Normal layer blends, no masks or animation tracks, and three unsupported-content layers each. These are corpus checks, not proof of full-fidelity conversion.

## Architecture decisions

### 1. Document model: heterogeneous, editable content

Keep a stable document graph in the engine. Introduce a native content abstraction with explicit variants:
- Raster tiles: premultiplied linear-light pixel payloads with declared color profile and precision.
- Vector scene: paths, pressure-aware stroke outlines/centerlines, fill rules, brush/material references, transforms, and stable object IDs.
- Hybrid/lossless paint surface: vector-like retained edit operations plus a deterministic raster cache. It supports raster brush operations, blending, masks, liquify and filters. The cache is disposable; retained operations are authoritative until an operation cannot be represented losslessly, at which point the layer records an explicit rasterization boundary.
- Adjustment/filter nodes and masks remain non-destructive where possible.

A vector layer remains truly vector and is not forced through raster-only operations. The CLIP import path now creates a hybrid source layer: the raster projection remains immediately usable, while original vector-record bytes and the complete source file (when within the import size cap) are retained. This is a preservation boundary, not yet a full vector editor: individual path selection, velocity/stabilization replay, and brush-style reconstruction still need implementation. The imported raster projection is at document resolution; scale-aware rerendering from the retained source records is not wired yet, so do not upscale that cached projection and call it lossless. Liquify and arbitrary pixel warps use explicit non-destructive deformation/filter nodes or a declared rasterization boundary; do not claim arbitrary pixel warps are mathematically vector-lossless.

### 2. Rendering and high-resolution performance

Use a tile/region renderer with demand-driven residency:
- Keep document coordinates independent of display pixels and DPI.
- Tile work is bounded and scheduled by visible region, edit region, and prefetch priority.
- Keep CPU authoritative tile/operation data separate from GPU cache residency.
- Track memory budgets, tile states, generations, and in-flight work; stale jobs must not overwrite newer edits.
- Use mip/LOD caches for display, while preserving full-resolution source data.
- Use incremental dirty-region compositing; avoid full-canvas readback for ordinary edits.
- Define deterministic CPU reference implementations for blend/composite and operation replay; GPU output is compared within documented tolerances.
- Instrument frame time, tile upload/readback bytes, peak CPU/GPU residency, cache hit rate, and save/open latency at representative 4K, 8K, 16K and high-DPI workloads.

### 3. Compositing

Retain the existing 27 blend modes and verify every mode in the layer UI and GPU path. The CLIP corpus confirms all 47 layers in Illustration1clip and all five layers in Illustration2clip use Normal; these samples do not validate non-Normal equations. CLIP Glow Dodge and Add (glow) currently map to Color Dodge and Add with an approximation warning. CLIP Brightness maps to Luminosity with an approximation warning. Solo is layer visibility/isolation, not a pixel blend equation. Add distinct modes only with published equations, alpha/coverage semantics, CPU reference vectors, WGSL implementation, and cross-backend golden tests.

### 4. HanaCreate file format

Create a versioned .hanacreate package with:
- a canonical manifest (format version, document UUID, dimensions, DPI, color profile, feature flags, object table);
- independently addressable content objects (raster tiles, vector scene chunks, operation logs, thumbnails, previews, references and optional time-lapse);
- per-object content digests, bounded lengths, explicit codecs and strict parser limits;
- deterministic serialization and golden fixtures;
- atomic save generations and crash-safe publication;
- forward-compatible unknown optional records and fail-closed unknown required features;
- an importer for existing .procraft/Procreate-compatible projects and explicit export where fidelity is representable.

Do not use the current HVS v2 reader as a writer. Define a storage adapter so a future qualified HVS writer can provide object storage/deduplication without changing the HanaCreate document schema. Reuse HVS hashing/verification primitives only through a narrow adapter after license/API review and test coverage. Never mix HVS v2 and HVFS identities or claim that one is the other.

### 5. Rust and Zig responsibilities

Rust owns the document model, editing semantics, brush behavior, serialization, deterministic CPU reference paths, and safe public API. Zig is the application/build orchestration and selected platform/rendering integration layer where a measured benefit exists. Avoid duplicating document business logic in Zig. Any Rust/Zig boundary is versioned, typed, bounded, and covered by ABI conformance tests.

### 6. UI direction

Preserve the existing UI hierarchy, controls, and liquid-glass surfaces. Treat the current interface as the baseline, not a redesign target. Refine the existing glass through motion: animate surface reveal, backdrop blur, tint, edge highlights, elevation, and small positional settling on open, close, focus, and selection. Reuse the existing motion system, keep drawing and stylus input paths free of continuous visual effects, and provide reduced-motion and reduced-transparency behavior. Keep the canvas visually dominant. The Android tablet is the first interaction and performance target.

## Implementation sequence

1. Android tablet validation: build and install on the connected arm64 tablet, verify S Pen pressure/tilt, canvas latency, memory behavior, and file-open flows.
2. Clip Studio Paint import into the current native artwork storage, with explicit warnings and representative fixture coverage.
3. Document graph and hybrid layer operation model; add round-trip and undo/redo invariants.
4. HanaCreate v1 manifest/object package, parser limits, deterministic fixtures, atomic saves, and ProCraft import/export.
5. New blend equations and solo/brightness UI semantics with CPU/GPU parity tests.
6. Vector/brush tool integration and hybrid-layer raster operations.
7. High-resolution tile scheduler, memory admission, prefetch and performance benchmarks.
8. Liquid-glass motion refinement using the existing UI: a restrained animated sheen now follows the existing frost and popover reveal; screenshot and accessibility checks remain.
9. HVS storage adapter only when a writer and its qualification gates exist.

## Acceptance gates

- No data loss when opening, editing, saving, reopening, undoing, or importing/exporting supported documents.
- Every advertised blend mode has a CPU oracle, WGSL parity fixture, UI entry, and serialization round trip.
- Vector and hybrid content survives save/open with stable IDs and brush parameters.
- High-resolution benchmarks report p50/p95 frame time, peak memory and bytes moved; improvements are measured against the baseline.
- Fuzz/property tests cover package parsing, malformed lengths, truncated objects, duplicate IDs, invalid references and decompression limits.
- Desktop visual changes include deterministic screenshots and keyboard, pen and reduced-transparency checks.
## Reference behavior

Krita's vector layer is a distinct shape layer: shapes retain editable geometry and their own z-order, render with antialiasing, and can be rasterized explicitly. HanaCreate's long-term hybrid layer should follow that separation: vector source and paint projection coexist, while rasterization is an explicit destructive boundary. See the [Krita vector-layer manual](https://docs.krita.org/en/reference_manual/layers_and_masks/vector_layers.html) and [layer commands](https://docs.krita.org/en/reference_manual/main_menu/layers_menu.html).

The [Clip Studio Paint blend-mode manual](https://help.clip-studio.com/en-us/manual_en/180_layers/Blending_modes.htm) describes Glow Dodge as stronger than Color Dodge and Add (Glow) as stronger than Add, but does not publish their pixel equations. Keep those modes visibly marked as approximations until their alpha/coverage behavior has reference vectors; do not alias them silently.
