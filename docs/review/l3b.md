# L3-B review (fail)

L3-B delivers the wgpu renderer: instanced cuboids read as instance-rate vertex attributes, cutout and sorted
translucent passes with the depth pre-pass of app 8.3, voxel face pools with CPU shadows, mesh objects, the
analytic sub-grid, overlay lines, the outline mask and composite, MSAA off or 4x, atlas pages with partial
updates, the PNG render with a non-blocking read-back, error scopes around every resource and a device-lost
path. `cargo clippy -p cuboid-render --all-targets -- -D warnings`, `cargo test -p cuboid-render` (29 passed,
1 ignored, on WARP), `cargo fmt -p cuboid-render -- --check` and `cargo xtask layering` pass. The benchmark
reproduces on the RTX 4060 (Vulkan): 10 000 elements and 2 088 960 voxel faces, 18 MiB on the device, upload
11 ms, frame median 2.06 ms, worst 3.61 ms, CPU encode 0.21 ms. I found no panic, abort or hang in the frame
loop. Indexing is clipped to the shadows and to buffer capacities, every texture and buffer size is above
zero, minimised and oversized views keep their old picture, and calls on a lost device are no-ops in wgpu 30
(lost-device errors are not reported to scopes or handlers). A panicking PNG worker turns into an error.
Transparency order, reversed Z, culling and handedness, the face table of tools 4.3, AO, voxel colour
packing and the element outline match the spec. The view textures (`Rgba8Unorm` holding gamma values) match
what egui 0.37 expects. The package fails on four items. Uploads that finish after their frame never reach
the views, which also leaves the picture broken after a device rebuild. MSAA cuts seams into every textured
edge. Meshes get no outline. A mesh transform change rebuilds the whole mesh with its texture every frame.

## Blocking

1. `crates/cuboid-render/src/composite.rs:51` with `crates/cuboid-render/src/gpu.rs:399` (`sync`).
   - What is wrong: a view renders again only when its `FrameKey` changed or its texture is new. `sync` spreads
     leftovers over later frames, but it never invalidates the view keys. In those later frames the scene key
     does not change, so what they upload never reaches the view textures. The views stay stale until the
     camera, the selection or the scene changes.
   - Scenario A (rebuild): a model with a voxel world and one 2048 atlas page loses its device. After
     `rebuild`, each shadow is queued as one item per pool or page. The first `sync` sends the element run, atlas
     page 0 (16 MiB, over the 8 MiB budget, but the first item of each kind always goes) and voxel pool 0.
     Pools 1 to 5 wait because the byte budget is used up. The views render in that frame with five face
     directions missing. The next calls upload the rest, but the keys did not change, so the views stay broken
     until the user moves the camera. Meshes after the first and atlas pages after the first behave the same.
     So `rebuild` does not really recover the picture.
   - Scenario B (normal use): opening a document whose first extracts exceed 8 MiB or 4 ms (all instances,
     8 MiB of atlas rectangles, every mesh, a voxel batch). Or adding the texture that grows the atlas from 2
     to 4 layers: `Atlas::grow` queues every page in full and one page goes per frame. The last leftovers show
     only after the next camera move. Until then chunks are missing, or faces on a page that is still empty
     sample alpha 0, get discarded and show as holes.
   - Fix: when a `sync` call uploads anything, clear `key` on all four `ViewSlot`s. Or keep an upload counter and
     store it next to the key, and render again when it changed. Add a test: sync with a small budget, render,
     sync again with nothing new, render with the same key, and expect the new pixels.

2. `crates/cuboid-render/src/shaders/elements.wgsl:132` and `:144`, with `crates/cuboid-scene/src/extract.rs:780`
   and `crates/cuboid-scene/src/atlas_layout.rs:19`.
   - What is wrong: the face rectangle from the scene is exactly the atlas entry, and the one-texel padding
     around each entry is transparent. `uv` uses the default centre interpolation. With 4x MSAA a pixel on a
     face edge is shaded at the pixel centre even when that centre lies outside the face. The UV then lands just
     past the rectangle, nearest sampling picks a transparent padding texel, and `fs_cutout` discards the
     fragment (`:159`). The samples that the face covers in that pixel keep the clear colour or whatever is
     behind.
   - Scenario: turn on "Enable anti-aliasing" and look at any cuboid. Faces without a texture use the "error"
     texture, so every element is affected. Every crease and silhouette shows a dotted dark seam in the
     background colour, and the anti-aliased fringe on the outer side of each edge is lost. Translucent faces
     lose their edge pixels in both passes. PNG renders with MSAA on show the same seams. Without MSAA it
     happens where a pixel centre sits exactly on the far edge of the rectangle (orthographic views at some zoom
     levels) or where float error pushes the UV over. The tests miss it because `SOLID`
     (`crates/cuboid-render/src/tests.rs:134`) is inset by one texel.
   - Fix: pass the face rectangle to the fragment stage as a flat varying. Clamp the UV to it, inset by half a
     texel: `clamp(uv, tl + 0.5 / page, br - 0.5 / page)`, with the page edge from `textureDimensions`. Adding
     `@interpolate(perspective, centroid)` on `uv` helps as well. Add an MSAA test with an exact-entry rectangle
     and transparent padding that checks a pixel on a crease.

3. `crates/cuboid-render/src/composite.rs:151`.
   - What is wrong: the outline mask pass draws only `DrawTarget::Elements` items. The scene puts selected and
     hovered meshes into `ViewDrawList.outline` as `DrawTarget::Mesh` items (`crates/cuboid-scene/src/drawlist.rs:575`),
     and the renderer skips them.
   - Scenario: select or hover a primitive sphere or an imported OBJ. No orange or white band appears (app
     8.7). The wireframe box of app 8.8 has 2 % alpha, so apart from the gizmo the selection cannot be seen.
   - Fix: add a mesh mask pipeline (an `fs_mask` in `meshes.wgsl` with the 0.5 alpha test, 1 sample, no depth)
     and draw the mesh outline items with their sub-mesh bind groups. Meshes have no hover bit, so the outline
     contract needs the colour per item. Add a `hovered` flag (or a separate hovered item) to the outline in
     cuboid-scene, and pass the colour through a small uniform or a second pipeline.

4. `crates/cuboid-render/src/meshes.rs:180` (`apply`) and `:233` (the byte cost).
   - What is wrong: every `MeshUpdate` drops the mesh's device resources (`gpu: None`) and queues a full
     re-creation: vertex data with normals, index buffers, the texture with a full `write_texture`, uniforms and
     bind groups. The scene sends a `MeshUpdate` whenever the mesh `Arc` changes
     (`crates/cuboid-scene/src/extract.rs:489`), which includes a move, rotate, scale, tint or hide. The byte cost
     leaves the texture out, so the budget does not see it.
   - Scenario: drag an imported mesh with a 4096 x 4096 texture with the gizmo. Each frame uploads 64 MiB of
     texture plus the buffers and allocates a new texture, far past 16 ms. Or drag two meshes of about 100 k
     vertices each: the second does not fit the 8 MiB budget and is not drawn in that frame. The two then take
     turns, so they flicker for the whole drag.
   - Fix: in `apply`, when `geometry`, `texture` and `submeshes` are the same `Arc`s as the stored ones, only
     rewrite the sub-mesh uniforms (model, normal matrix, colour) with `write_buffer`, update `hidden`, and keep
     `gpu`. For a real geometry change, keep drawing the old `gpu` until the new one exists. Count the texture
     bytes in the cost, and skip duplicate ids in `pending` (today a mesh queued twice is created twice).

## Nonblocking

- `gpu.rs:400`: `sync` returns before it applies the extract while the device is lost. The scene has drained
  those changes, so edits made between the loss and `rebuild` never reach the shadows, and the rebuilt picture
  shows the old state of those objects. Apply to the shadows always; skip only the growth and the uploads.
- `gpu.rs:170-173`: `Gpu::new` moves `old` in and then returns early with `?` when an adopt fails. That happens on
  out-of-memory, or when the new device has a smaller `max_buffer_size` than a pool needs (a fallback to WARP or
  an integrated GPU). Every shadow is then dropped and the scene never resends, so the views stay empty. Keep the
  shadows and mark the pool as starved instead.
- `gpu.rs:201-207` and `:369-379`: when pipeline creation fails, `pipes` stays `None` and nothing retries it.
  This can happen during driver recovery. The views then stay blank until MSAA is toggled. Retry in
  `render_views`, rate-limited.
- `elements.rs:88`: an extract with `instance_count: 0` (a default extract, as the tests use to drain the queue)
  truncates every instance. Make the count explicit (`Option`) or document it on `sync`.
- `gpu.rs:496-501` with `:361-385`: after the budget drops MSAA, `settings().msaa` is false. An app that pushes
  its preferences again (MSAA on) recompiles all 13 pipelines, reallocates the attachments and drops MSAA
  again. Keep the wanted value apart from the effective one, and retry only after the budget changed.
- `gpu.rs:437-441`: the elements, the atlas and the voxel pools each check their growth against the same `room`,
  so one call can overshoot the budget by up to three times. Subtract as you go.
- CPU shadows are full copies of what was uploaded, except meshes, which share the document's `Arc`s. Voxel
  pools take 8 bytes per record at the scene's capacity (up to twice the live faces), so 1 M faces cost 8 to
  16 MB. That is fine. Atlas pages take 16 MiB per 2048 page even when the page is nearly empty, which doubles
  texture RAM. The voxel shadow grows to the scene's capacity before the GPU budget check (`voxels.rs:80-88`,
  up to 512 MiB per pool), so a refused pool still costs its RAM. Check the budget before resizing.
- `atlas.rs:177-183`: growing the layers re-uploads every page from the shadows; `copy_texture_to_texture` of
  the old layers avoids that. `atlas.rs:227-231` copies each rectangle into a temporary `Vec`; `write_texture`
  can read the page directly with an offset and `bytes_per_row = size * 4`.
- `atlas.rs:94` and `:270`: the atlas starts as a one-layer texture with a `D2Array` view. On the GL backend
  (the `gles` feature is enabled) wgpu-hal creates a one-layer texture as `TEXTURE_2D`, and the array view does
  not work (wgpu issues 1574 and 1614). Create at least two layers.
- `readback.rs:99-103`: the mapped range is copied to a `Vec` on the UI thread. That is 256 MiB for an 8192 x
  8192 render, a long stall. Move the buffer to the worker (`wgpu::Buffer` is `Send`) and copy there. Mapping
  on the first poll after recording breaks the submit if the app polls before it submits. Mapping one poll
  later makes that misuse harmless.
- `offscreen.rs:145-185`: PNG targets are outside the memory budget (about 1.3 GiB at 8192 x 8192 with MSAA).
  Every job writes the same uniform slot (`offscreen.rs:129`), so two jobs in one encoder both use the last
  camera.
- `meshes.rs:275-305`: a mesh texture above the device limit fails validation, and the mesh is not drawn. Scale
  it down as tools 5.2 and 5.10 do for atlas images.
- `meshes.rs:480`: the front face is fixed to clockwise. If a mesh model matrix can be mirrored (negative scale
  or a flip), its faces get culled the wrong way. Flip the front face per mesh when the determinant is
  negative.
- `gpu.rs:489-492`: the shared attachments only grow. After one maximised 4K view with MSAA they keep about
  330 MB. Bounded, but consider shrinking them when the largest view has stayed smaller for a while.
- `composite.rs:268-270`: the atlas bind group and the instance buffer are set again for every element item. A
  model with many translucent elements pays six commands per element. Set them only when the item kind
  changes.
- `voxels.rs:143-167`: the scene reuses a freed pool block at once. When the renderer defers the new owner's
  upload, the buffer still holds the previous chunk's records, and they draw for those frames as ghost faces.
  Once item 1 is fixed this lasts only while the backlog drains.
- `overlays.rs:174`: selection boxes are depth-tested; app 8.8 draws them on top. At 2 % alpha this barely
  shows.
- `tests.rs:223`: the benchmark renders one 96 x 96 view with contiguous ranges (six voxel draws) and MSAA off.
  That measures vertex work only. Also measure 1920 x 1080, MSAA on, four views and scattered chunk ranges
  before L3 exits.
- `sync` takes a `queue` and `poll_readbacks` a `device`, although the renderer holds both. Passing the old
  queue after `rebuild` writes to the wrong device. Use the held handles.
- `backgrounds.rs`, `shadows.rs`, `wireframe.rs` and `preview.rs` are one-line stubs, and `DrawTarget::Background`
  is ignored (`composite.rs:306`). These are outside the L3 scope of roadmap 4, but the scene already emits
  background items.

## Integration notes

- `crates/cuboid-app/src/render_glue.rs:18` repeats `AdapterCaps::detect`; call it instead. Nothing calls
  `rebuild` yet. When `take_device_lost` fires: recreate the egui `RenderState` and the surface, call
  `rebuild(device, queue, caps)` with the new ones, and pass that queue to every later `sync`. Until the
  lost-device extract item is fixed, skip `extract` while `device_lost()` is true, so the extractor keeps its
  dirty state.
- Frame order: `Extractor::extract`, then `Renderer::sync` every frame (also when the extract is empty, because
  leftovers drain only inside `sync`), then `compile_draw_list` per view, `render_views`, the egui pass, the
  submit, and `poll_readbacks` after the submit. Always pass the extractor's output to `sync`, never a
  hand-made or default extract.
- Keep one `Extractor` per renderer for the whole session. A new extractor per document never sends
  `removed_meshes` for the old meshes, so their buffers and textures, the atlas pages and the chunk records stay
  in the renderer. Otherwise add a reset to `Renderer`.
- `ViewParams`: `size` in physical pixels and equal to the `FrameKey` size; `view_index` equal to the scene's
  `ViewId::index`, because the instance view masks use it; the array in quadrant order; `None` for a hidden
  view.
- Register each `ViewTexture` once with `register_native_texture` (nearest filtering). Register it again when
  `generation` changes (resize, MSAA switch, rebuild). Draw it at its physical size.
- PNG jobs: build the extract with the live extractor and `ExtractBudget::UNLIMITED`. Record
  `start_png_render` in its own encoder and submit it apart from the views, because the job's uploads would
  otherwise reach the view passes of the same submit. Start one job per submit. Use a `camera.view_index`
  from 0 to 3 (bit 4 of the instance flags is the hover bit), and keep hidden elements out of the PNG draw
  list. With anti-aliasing the output edge is at most `max_texture_size / 2`, which is 4096 under egui's
  default limit of 8192.
- `Renderer::new` and `rebuild` install the device-lost callback and the uncaptured-error handler on the shared
  device, replacing any earlier ones. The app must not install its own afterwards, or it must forward to the
  renderer's.
- Call `set_settings` only when the preferences change (it resets every view key). Compare against the app's
  own copy, not `settings()`, which may report MSAA dropped by the budget.
- Show `Health.degraded` and `Health.last_error` in the status bar. They are the only signal that an upload
  or MSAA was refused.
- `ExtractBudget` and `UploadBudget` both default to 8 MiB and 4 ms, so a heavy frame can spend about 8 ms of
  CPU before rendering. Lower one of them if frames during loads go over 16 ms.
