//! Pictures of models: a 160 px render of a model file with the offscreen PNG path of the
//! renderer. One model at a time and a little each frame: the file is read on the library thread,
//! the scene is extracted and uploaded within the usual frame budgets, and the draw and the
//! read-back take one frame each. The rig has a renderer of its own, so the live views are never
//! touched; it is dropped when the queue has been empty for a while, so the GPU memory and the
//! pipelines of the second renderer do not stay for an idle program.

use std::time::{Duration, Instant};

use cuboid_library::thumb::THUMB_EDGE;
use cuboid_model::image::{Image, RgbaBuf};
use cuboid_model::{Document, ObjRef, RenderPrefs};
use cuboid_render::{AdapterCaps, PngJob, ReadbackId, Renderer, UploadBudget, ViewParams};
use cuboid_scene::{
    ExtractBudget, Extractor, OverlayState, Rect, RenderExtract, SceneIndex, ViewSet,
    compile_draw_list, frame_bounds,
};

use crate::flows::{Opened, open_file};
use crate::jobs::JobHandle;
use crate::library::{LibraryService, RenderRequest};
use crate::state::State;

/// Models above this size are not drawn for a picture.
const MAX_MODEL_BYTES: u64 = 32 << 20;
/// Time with nothing to do before the renderer is released.
const IDLE_RELEASE: Duration = Duration::from_secs(10);
/// The main perspective view of the scene's view set.
const MAIN: usize = 3;

enum Step {
    /// The model is being read.
    Loading(JobHandle<Opened>),
    /// The scene is being extracted and uploaded.
    Drawing(Box<Scene>),
    /// The picture is being read back.
    Reading(ReadbackId),
}

struct Scene {
    doc: Document,
    extractor: Extractor,
}

struct Active {
    request: RenderRequest,
    step: Step,
}

/// Draws the pictures the library service asks for.
#[derive(Default)]
pub struct ThumbRig {
    renderer: Option<(Renderer, wgpu::Device)>,
    active: Option<Active>,
    /// Since when the rig has had nothing to do while it holds a renderer.
    idle_since: Option<Instant>,
}

impl ThumbRig {
    /// True while a picture is being made or one waits.
    pub fn busy(&self, library: &LibraryService) -> bool {
        self.active.is_some() || library.has_renders()
    }

    /// How long until the idle renderer is released; the caller asks for a frame by then.
    pub fn release_in(&self, now: Instant) -> Option<Duration> {
        let since = self.idle_since?;
        Some(IDLE_RELEASE.saturating_sub(now.saturating_duration_since(since)))
    }

    /// One frame of work. `device` and `queue` are the shell's; the rig keeps its own renderer
    /// on them, drops it when the device changes and after [`IDLE_RELEASE`] without work.
    pub fn step(
        &mut self,
        state: &mut State,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        caps: &AdapterCaps,
    ) {
        self.work(state, device, queue, caps);
        if self.active.is_some() || self.renderer.is_none() {
            self.idle_since = None;
            return;
        }
        self.idle_since.get_or_insert(state.now);
        if self.release_in(state.now) == Some(Duration::ZERO) {
            self.renderer = None;
            self.idle_since = None;
        }
    }

    fn work(
        &mut self,
        state: &mut State,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        caps: &AdapterCaps,
    ) {
        if self
            .renderer
            .as_ref()
            .is_some_and(|(renderer, own)| own != device || renderer.device_lost())
        {
            self.renderer = None;
            if let Some(active) = self.active.take() {
                state.library.render_done(active.request, None);
            }
        }
        let Some(active) = self.active.take() else {
            self.start(state);
            return;
        };
        let Active { request, step } = active;
        let next = match step {
            Step::Loading(mut handle) => match handle.poll() {
                None => Some(Step::Loading(handle)),
                Some(Ok(opened)) => Some(Step::Drawing(Box::new(Scene {
                    doc: opened.doc,
                    extractor: Extractor::default(),
                }))),
                Some(Err(error)) => {
                    log::info!("no picture for {}: {error}", request.source.display());
                    None
                }
            },
            Step::Drawing(scene) => {
                let (renderer, _) = self
                    .renderer
                    .get_or_insert_with(|| (Renderer::new(device, queue, caps), device.clone()));
                let prefs = state.editor.core.env_mut().render.clone();
                draw(renderer, device, queue, scene, &prefs)
            }
            Step::Reading(id) => {
                let Some((renderer, _)) = self.renderer.as_mut() else {
                    state.library.render_done(request, None);
                    return;
                };
                match renderer
                    .poll_readbacks(device)
                    .into_iter()
                    .find(|(got, _)| *got == id)
                {
                    None => Some(Step::Reading(id)),
                    Some((_, Ok(buf))) => {
                        state.library.render_done(request, Some(picture(buf)));
                        return;
                    }
                    Some((_, Err(error))) => {
                        log::info!("no picture for {}: {error}", request.source.display());
                        None
                    }
                }
            }
        };
        match next {
            Some(step) => self.active = Some(Active { request, step }),
            None => state.library.render_done(request, None),
        }
    }

    fn start(&mut self, state: &mut State) {
        let Some(request) = state.library.take_render() else {
            return;
        };
        let too_big = std::fs::metadata(&request.source).is_ok_and(|m| m.len() > MAX_MODEL_BYTES);
        let env = state.editor.core.env_mut().clone();
        let source = request.source.clone();
        let handle = state
            .library
            .io()
            .filter(|_| !too_big)
            .map(|io| io.submit(move |ctx| open_file(&source, None, &env, ctx)));
        match handle {
            Some(handle) => {
                self.active = Some(Active {
                    request,
                    step: Step::Loading(handle),
                });
            }
            None => state.library.render_done(request, None),
        }
    }
}

/// The picture as an image for the PNG encoder.
fn picture(buf: RgbaBuf) -> Image {
    Image::from_rgba(buf)
}

/// Extracts and uploads what the budgets allow; once everything is up, draws the model and
/// starts the read-back.
fn draw(
    renderer: &mut Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mut scene: Box<Scene>,
    prefs: &RenderPrefs,
) -> Option<Step> {
    let Scene { doc, extractor } = &mut *scene;
    let extract = extractor.extract(doc, prefs, &ExtractBudget::default());
    renderer.sync(queue, &extract, UploadBudget::default());
    if extractor.has_leftovers() || renderer.needs_frame() {
        return Some(Step::Drawing(scene));
    }
    let job = png_job(&mut scene, prefs);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("cuboid_thumb"),
    });
    let id = renderer.start_png_render(&mut encoder, &job);
    queue.submit([encoder.finish()]);
    Some(Step::Reading(id))
}

/// The draw list and camera of the picture: the main perspective view framed on the model, no
/// grid and no overlays. The extract is empty because the renderer already holds the scene.
fn png_job(scene: &mut Scene, prefs: &RenderPrefs) -> PngJob {
    let doc = &scene.doc;
    let mut index = SceneIndex::default();
    index.sync(doc, &doc.journal);
    let objects: Vec<ObjRef> = doc
        .elements
        .ids()
        .iter()
        .map(|id| ObjRef::Element(*id))
        .chain(doc.meshes.ids().iter().map(|id| ObjRef::Mesh(*id)))
        .collect();
    let mut views = ViewSet::default();
    let ViewSet {
        views: list, pivot, ..
    } = &mut views;
    if let Some(bounds) = index.bounds_of(&objects) {
        frame_bounds(&mut list[MAIN].camera, pivot, bounds.min, bounds.max, 1.0);
    }
    let edge = THUMB_EDGE as f32;
    list[MAIN].rect = Rect {
        x: 0.0,
        y: 0.0,
        w: edge,
        h: edge,
    };
    scene.extractor.cache_mut().set_overlay(OverlayState {
        grid: false,
        ..OverlayState::default()
    });
    let main = &views.views[MAIN];
    PngJob {
        extract: RenderExtract::default(),
        list: compile_draw_list(main, scene.extractor.cache(), prefs),
        camera: ViewParams::from_camera(&main.camera, 3, (THUMB_EDGE, THUMB_EDGE)),
        antialias: true,
    }
}
