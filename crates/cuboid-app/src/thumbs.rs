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
use cuboid_render::{
    AdapterCaps, DeviceWatch, PngJob, ReadbackId, Renderer, UploadBudget, ViewParams,
};
use cuboid_scene::{
    ExtractBudget, Extractor, OverlayState, Rect, RenderExtract, SceneIndex, ViewSet,
    compile_draw_list, frame_bounds,
};

use crate::crash;
use crate::flows::{Opened, open_file};
use crate::jobs::{JobError, JobHandle};
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
    /// Makes the next drawing panic, to test that the frame does not notice.
    #[cfg(test)]
    pub fail_draw: bool,
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

    /// One frame of work. `device`, `queue` and `watch` are the shell's; the rig keeps its own
    /// renderer on them, drops it when the device changes and after [`IDLE_RELEASE`] without work.
    pub fn step(
        &mut self,
        state: &mut State,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        caps: &AdapterCaps,
        watch: &DeviceWatch,
    ) {
        self.work(state, device, queue, caps, watch);
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
        watch: &DeviceWatch,
    ) {
        if self
            .renderer
            .as_ref()
            .is_some_and(|(_, own)| own != device || watch.lost())
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
            // A model that makes the extractor or the renderer panic costs its picture. The panic
            // boundary of the frame would restore the document of the user instead.
            Step::Drawing(scene) => {
                let drawn = crash::catch("drawing a model picture", || {
                    let (renderer, _) = self.renderer.get_or_insert_with(|| {
                        (
                            Renderer::with_watch(device, queue, caps, watch),
                            device.clone(),
                        )
                    });
                    let prefs = state.editor.core.env_mut().render.clone();
                    #[cfg(test)]
                    assert!(
                        !std::mem::take(&mut self.fail_draw),
                        "the picture failed on purpose"
                    );
                    draw(renderer, device, queue, scene, &prefs)
                });
                drawn.unwrap_or_else(|| {
                    self.renderer = None;
                    None
                })
            }
            Step::Reading(id) => {
                let polled = self.renderer.as_mut().and_then(|(renderer, _)| {
                    crash::catch("reading a model picture", || {
                        renderer.poll_readbacks(device)
                    })
                });
                let Some(polled) = polled else {
                    self.renderer = None;
                    state.library.render_done(request, None);
                    return;
                };
                match polled.into_iter().find(|(got, _)| *got == id) {
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
        let env = state.editor.core.env_mut().clone();
        let source = request.source.clone();
        // The size is read in the job, on the library I/O thread: the UI thread touches no file.
        let handle = state.library.io().map(|io| {
            io.submit(move |ctx| {
                let bytes = std::fs::metadata(&source)
                    .map_err(|error| JobError::Failed(error.to_string()))?
                    .len();
                if bytes > MAX_MODEL_BYTES {
                    return Err(JobError::Failed(format!(
                        "{bytes} bytes is too large for a picture"
                    )));
                }
                open_file(&source, None, &env, ctx)
            })
        });
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

#[cfg(test)]
mod tests {
    use cuboid_core::{Command, ElementKind};
    use cuboid_settings::{Environment, Settings};
    use cuboid_shell::{EngineKeyEvent, FrameCx, FrameRequest, Headless, ShellApp, ShellEvent};

    use super::*;
    use crate::crash::PanicGuard;
    use crate::io_thread::IoThread;
    use crate::platform::Dirs;
    use crate::state::{Intent, Level};

    /// A state in a temporary folder whose document is saved to `tower.cubik`, which makes the
    /// library ask for a picture of it.
    fn state_with_a_saved_model(dir: &std::path::Path) -> State {
        let settings = Settings::first_run(&Environment {
            desktop_dir: String::new(),
            minecraft_dir: String::new(),
        });
        let mut state = State::new(
            Dirs::under(dir),
            settings,
            IoThread::spawn().expect("thread"),
            1252,
            Instant::now(),
        );
        state.run_command(Command::AddElement(ElementKind::Cube));
        state.dispatch(Intent::SaveAs(Some(dir.join("tower.cubik"))));
        let start = Instant::now();
        while state.doc_job.is_some() {
            state.step(Instant::now());
            assert!(start.elapsed() < Duration::from_secs(20), "the save hangs");
            std::thread::sleep(Duration::from_millis(2));
        }
        state
    }

    /// The frame of the app reduced to the rig, under the panic boundary of the real frame
    /// (without the refresh of the last good state, which the test wants to stay old).
    struct RigApp {
        state: State,
        rig: ThumbRig,
        guard: PanicGuard,
        watch: Option<DeviceWatch>,
        /// Panics that reached the frame boundary.
        panics: usize,
    }

    impl ShellApp for RigApp {
        fn engine_key(&mut self, _ev: EngineKeyEvent) {}

        fn window_event(&mut self, _ev: ShellEvent) {}

        fn frame(&mut self, cx: &mut FrameCx<'_>) -> FrameRequest {
            let gpu = cx.gpu;
            let caps = AdapterCaps::detect(&gpu.adapter, &gpu.device);
            let watch = self
                .watch
                .get_or_insert_with(|| DeviceWatch::new(&gpu.device));
            let Self {
                state, rig, guard, ..
            } = self;
            let now = cx.now;
            let finished = guard.run(|| {
                state.step(now);
                rig.step(state, &gpu.device, &gpu.queue, &caps, watch);
            });
            if let Err(panicked) = finished {
                self.panics += 1;
                self.state.recover_from_panic(&panicked);
            }
            FrameRequest::default()
        }
    }

    #[test]
    fn a_thumbnail_that_panics_does_not_restore_the_document() {
        let mut headless = match Headless::new((64, 64), 1.0) {
            Ok(headless) => headless,
            Err(error) => {
                assert!(
                    std::env::var_os("CI").is_none(),
                    "no wgpu adapter under CI: {error}"
                );
                eprintln!("SKIPPED (no wgpu adapter): {error}");
                return;
            }
        };
        let dir = tempfile::tempdir().expect("temp dir");
        let mut state = state_with_a_saved_model(dir.path());
        assert!(
            state.library.has_renders(),
            "the library asks for a picture"
        );
        // A change that the last good state (the empty model) does not hold: a restore would
        // drop it.
        state.run_command(Command::AddElement(ElementKind::Cube));
        let mut app = RigApp {
            state,
            rig: ThumbRig {
                fail_draw: true,
                ..ThumbRig::default()
            },
            guard: PanicGuard::default(),
            watch: None,
            panics: 0,
        };

        let start = Instant::now();
        loop {
            headless.frame(&mut app, &[]).expect("a frame");
            if !app.rig.busy(&app.state.library) {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "the rig hangs");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!app.rig.fail_draw, "the drawing was reached and failed");
        assert_eq!(app.panics, 0, "the panic stayed inside the rig");
        assert_eq!(app.state.editor.doc().elements.len(), 2);
        let errors = app.state.toasts.iter().filter(|t| t.level == Level::Error);
        assert_eq!(errors.count(), 0, "the user is told nothing");
        assert_eq!(app.state.autosave_seq, 0, "no recovery ran");
        assert!(!app.state.library.has_renders());
    }
}
