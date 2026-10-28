//! The PNG render of the render dialog, a few frames at a time.
//!
//! The picture is drawn with the offscreen path of the live renderer, whose scene is already
//! uploaded: the draw list and camera are built from the extractor of the window, the draw is
//! recorded once the uploads are done, and the read-back is polled each frame without blocking.
//! Colour backgrounds, PNG encoding and the atomic write run on the I/O thread.

use std::fs;
use std::path::{Path, PathBuf};

use cuboid_model::image::Image;
use cuboid_model::{Document, GuiLight, ObjRef};
use cuboid_render::ViewParams;
use cuboid_render::{ItemLight, PngJob, ReadbackId, RenderSettings as GpuSettings, ShadingMode};
use cuboid_scene::{
    Camera, OverlayState, Rect, RenderExtract, SceneIndex, StandardView, ViewId, compile_draw_list,
    frame_bounds, set_ortho, snap_view,
};
use cuboid_ui::{RenderBackground, RenderCamera, RenderRequest, RenderSettings, RenderShading};
use egui_wgpu::RenderState;
use glam::Vec3;

use super::Frontend;
use crate::jobs::{JobError, JobHandle};
use crate::state::State;

pub(super) enum Phase {
    /// The scene is still being extracted or uploaded.
    Waiting,
    /// The draw is recorded; the pixels are on their way back.
    Reading(ReadbackId),
    /// The I/O thread encodes and writes the file.
    Encoding(JobHandle<PathBuf>),
}

/// A picture being made.
pub(super) struct PngRender {
    settings: RenderSettings,
    path: PathBuf,
    phase: Phase,
}

/// The world box of a voxel model without elements: one block, in the internal frame (z runs
/// the other way).
const VOXEL_BOX: (Vec3, Vec3) = (Vec3::new(0.0, 0.0, -16.0), Vec3::new(16.0, 16.0, 0.0));

/// Makes the camera of a picture orthographic and frames it on the world box of everything the
/// model draws: its elements and meshes, or one block for a voxel model without elements. The
/// scale depends on the box alone, not on the way the camera looks at it, so the views of one
/// model are comparable. A model with nothing to draw is not framed.
pub(crate) fn frame_model(
    camera: &mut Camera,
    pivot: &mut Vec3,
    doc: &Document,
    index: &SceneIndex,
    aspect: f32,
) {
    set_ortho(camera, *pivot, true);
    let objects: Vec<ObjRef> = doc
        .elements
        .ids()
        .iter()
        .map(|id| ObjRef::Element(*id))
        .chain(doc.meshes.ids().iter().map(|id| ObjRef::Mesh(*id)))
        .collect();
    let bounds = index
        .bounds_of(&objects)
        .map(|b| (b.min, b.max))
        .or_else(|| doc.voxels.is_some().then_some(VOXEL_BOX));
    if let Some((min, max)) = bounds {
        frame_bounds(camera, pivot, min, max, aspect);
    }
}

/// Blends straight RGBA pixels over a flat colour and makes them opaque.
fn over_color(pixels: &mut [u8], color: [u8; 3]) {
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        for (channel, back) in pixel.iter_mut().zip(color) {
            let mixed = (u32::from(*channel) * alpha + u32::from(back) * (255 - alpha) + 127) / 255;
            *channel = u8::try_from(mixed).unwrap_or(u8::MAX);
        }
        pixel[3] = u8::MAX;
    }
}

impl Frontend {
    /// What the render dialog asks for.
    pub fn render_request(&mut self, state: &State, request: RenderRequest) {
        match request {
            RenderRequest::Start { settings, path } => {
                if self.png.is_some() {
                    self.render_failed("Another picture is being rendered.".to_owned());
                    return;
                }
                if state.editor.core.doc().elements.is_empty()
                    && state.editor.core.doc().meshes.is_empty()
                    && state.editor.core.doc().voxels.is_none()
                {
                    self.render_failed("The model is empty: nothing to draw.".to_owned());
                    return;
                }
                self.png = Some(PngRender {
                    settings,
                    path,
                    phase: Phase::Waiting,
                });
                self.more = true;
            }
            RenderRequest::Cancel => {
                if let Some(PngRender {
                    phase: Phase::Encoding(handle),
                    ..
                }) = self.png.take()
                {
                    handle.cancel();
                }
            }
        }
    }

    /// True when no picture is being made.
    #[cfg(test)]
    pub fn png_idle(&self) -> bool {
        self.png.is_none()
    }

    /// Opens the folder of a file in the system file manager. Tests only record the path.
    pub fn reveal(&mut self, path: &Path) {
        #[cfg(test)]
        self.revealed.push(path.to_owned());
        #[cfg(not(test))]
        open_folder(path);
        #[cfg(not(test))]
        let _ = self;
    }

    /// One frame of the picture that is being made.
    pub(super) fn step_png(&mut self, state: &mut State, gpu: &RenderState) {
        let Some(job) = self.png.take() else {
            return;
        };
        self.more = true;
        let PngRender {
            settings,
            path,
            phase,
        } = job;
        let next = match phase {
            Phase::Waiting => self
                .start_png(state, gpu, &settings)
                .map(|started| started.map_or(Phase::Waiting, Phase::Reading)),
            Phase::Reading(id) => self.read_png(state, gpu, id, &settings, &path),
            Phase::Encoding(mut handle) => match handle.poll() {
                None => Ok(Phase::Encoding(handle)),
                Some(Ok(path)) => {
                    self.ui.render_finished(Ok(path));
                    return;
                }
                Some(Err(JobError::Cancelled)) => return,
                Some(Err(error)) => Err(error.to_string()),
            },
        };
        match next {
            Ok(phase) => {
                self.png = Some(PngRender {
                    settings,
                    path,
                    phase,
                });
            }
            Err(text) => self.render_failed(text),
        }
    }

    /// Tells the dialog (or a toast) why no picture was made, and keeps the text for the tests.
    fn render_failed(&mut self, text: String) {
        log::warn!("cannot render the picture: {text}");
        if self.errors.len() < 10 {
            self.errors.push(text.clone());
        }
        self.ui.render_finished(Err(text));
    }

    /// The camera of the picture. The named cameras are orthographic views from a side of the
    /// model, framed on all of it; the current view is taken as it is.
    fn png_camera(&self, doc: &Document, choice: RenderCamera, size: (u32, u32)) -> Camera {
        if choice == RenderCamera::Current {
            return self.views.views[self.views.active.index()].camera;
        }
        let mut camera = Camera::default_for(ViewId::Main);
        let mut pivot = self.views.pivot;
        match choice {
            RenderCamera::Front => snap_view(&mut camera, pivot, StandardView::Front),
            RenderCamera::Side => snap_view(&mut camera, pivot, StandardView::Right),
            RenderCamera::Top => snap_view(&mut camera, pivot, StandardView::Top),
            RenderCamera::Iso | RenderCamera::Current => {}
        }
        let aspect = size.0.max(1) as f32 / size.1.max(1) as f32;
        frame_model(&mut camera, &mut pivot, doc, &self.index, aspect);
        camera
    }

    /// Records the draw once the scene is in the renderer. `Ok(None)` while it is not.
    fn start_png(
        &mut self,
        state: &mut State,
        gpu: &RenderState,
        settings: &RenderSettings,
    ) -> Result<Option<ReadbackId>, String> {
        if self.glue.is_none() {
            return Err("The renderer is not ready.".to_owned());
        }
        if self.extractor.has_leftovers()
            || self.glue.as_ref().is_some_and(|g| g.renderer.needs_frame())
        {
            return Ok(None);
        }
        let core = &mut state.editor.core;
        let prefs = core.env_mut().render.clone();
        let doc = core.doc();
        let size = (settings.size[0], settings.size[1]);
        let camera = self.png_camera(doc, settings.camera, size);

        let mut view = self.views.views[ViewId::Main.index()].clone();
        view.camera = camera;
        view.rect = Rect {
            x: 0.0,
            y: 0.0,
            w: size.0 as f32,
            h: size.1 as f32,
        };
        self.extractor.cache_mut().set_overlay(OverlayState {
            grid: false,
            ..OverlayState::default()
        });
        let list = compile_draw_list(&view, self.extractor.cache(), &prefs);
        let mut params = ViewParams::from_camera(&camera, ViewId::Main as u8, size);
        let item_light = match doc.props.minecraft.gui_light {
            GuiLight::Side => ItemLight::GuiSide,
            GuiLight::Front => ItemLight::GuiFront,
        };
        if settings.shading == RenderShading::ItemGui {
            params = params.with_item_light(item_light);
        }
        let job = PngJob {
            extract: RenderExtract::default(),
            list,
            camera: params,
            antialias: settings.msaa,
        };

        let Some(glue) = self.glue.as_mut() else {
            return Err("The renderer is not ready.".to_owned());
        };
        // The item lights apply to the Minecraft modes only; the other modes are switched for the
        // one draw. The shading only changes uniforms, which the draw reads while it is recorded.
        let live: GpuSettings = glue.renderer.settings();
        let forced = settings.shading == RenderShading::ItemGui
            && !matches!(
                live.shading,
                ShadingMode::MinecraftBlock | ShadingMode::MinecraftItem
            );
        if forced {
            glue.renderer.set_settings(GpuSettings {
                shading: ShadingMode::MinecraftItem,
                ..live
            });
        }
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cuboid_png"),
            });
        let id = glue.renderer.start_png_render(&mut encoder, &job);
        gpu.queue.submit([encoder.finish()]);
        if forced {
            glue.renderer.set_settings(live);
        }
        Ok(Some(id))
    }

    /// Polls the read-back; once the pixels are here the I/O thread takes over.
    fn read_png(
        &mut self,
        state: &State,
        gpu: &RenderState,
        id: ReadbackId,
        settings: &RenderSettings,
        path: &Path,
    ) -> Result<Phase, String> {
        let Some(glue) = self.glue.as_mut() else {
            return Err("The renderer is not ready.".to_owned());
        };
        let finished = glue.renderer.poll_readbacks(&gpu.device);
        let Some((_, result)) = finished.into_iter().find(|(got, _)| *got == id) else {
            return Ok(Phase::Reading(id));
        };
        let mut buf = result.map_err(|error| error.to_string())?;
        let background = settings.background;
        let target = path.to_owned();
        let handle = state.io.submit(move |ctx| {
            ctx.progress(0.1);
            if let RenderBackground::Color(color) = background {
                over_color(&mut buf.pixels, color);
            }
            let bytes = cuboid_model::image::encode_png(&Image::from_rgba(buf))
                .map_err(|error| JobError::Failed(error.to_string()))?;
            ctx.check()?;
            if let Some(dir) = target.parent().filter(|dir| !dir.as_os_str().is_empty()) {
                fs::create_dir_all(dir).map_err(|error| JobError::Failed(error.to_string()))?;
            }
            cuboid_io::write_atomic(&target, &bytes)
                .map_err(|error| JobError::Failed(error.to_string()))?;
            Ok(target)
        });
        Ok(Phase::Encoding(handle))
    }
}

/// Shows the file in the system file manager (its folder when the path is a folder).
#[cfg(not(test))]
fn open_folder(path: &Path) {
    use std::process::Command;

    let folder = path.is_dir();
    let result = if cfg!(windows) {
        let mut command = Command::new("explorer.exe");
        if folder {
            command.arg(path);
        } else {
            command.arg(format!("/select,{}", path.display()));
        }
        command.spawn()
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        if !folder {
            command.arg("-R");
        }
        command.arg(path).spawn()
    } else {
        let dir = if folder { Some(path) } else { path.parent() };
        Command::new("xdg-open")
            .arg(dir.unwrap_or_else(|| Path::new(".")))
            .spawn()
    };
    if let Err(error) = result {
        log::warn!("cannot open the folder of {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use cuboid_core::{Command, Compat, Core, ElementKind};
    use cuboid_model::TextureList;

    use super::*;

    /// The camera of a view of the document, framed by `frame_model`.
    fn framed(doc: &Document, view: StandardView) -> (Camera, Vec3) {
        let mut index = SceneIndex::default();
        index.sync(doc, &doc.journal);
        let mut camera = Camera::default_for(ViewId::Main);
        let mut pivot = Vec3::new(8.0, 8.0, -8.0);
        snap_view(&mut camera, pivot, view);
        frame_model(&mut camera, &mut pivot, doc, &index, 1.0);
        (camera, pivot)
    }

    #[test]
    fn every_view_of_a_model_is_framed_at_one_scale_on_the_middle_of_its_box() {
        let mut core = Core::new(Compat::port(), 0);
        assert!(
            core.execute(Command::AddElement(ElementKind::Cube))
                .is_done()
        );
        let (front, centre) = framed(core.doc(), StandardView::Front);
        let (top, top_centre) = framed(core.doc(), StandardView::Top);
        assert!(front.ortho && top.ortho);
        assert!(front.ortho_size > 0.9 && front.ortho_size < 1.1);
        assert!((front.ortho_size - top.ortho_size).abs() < 1e-5);
        assert_eq!(centre, top_centre);
        // The camera looks at the middle of the box from the side its view names.
        assert!(
            (front.pos + front.forward() * (centre - front.pos).dot(front.forward()) - centre)
                .length()
                < 1e-4
        );
        assert!(front.forward().z > 0.99 && top.forward().y < -0.99);
    }

    #[test]
    fn a_model_without_anything_to_draw_is_not_framed() {
        let doc = Document::new(TextureList::default());
        let (camera, pivot) = framed(&doc, StandardView::Front);
        assert!(camera.ortho, "the camera is still made orthographic");
        assert_eq!(pivot, Vec3::new(8.0, 8.0, -8.0));
    }

    #[test]
    fn a_colour_background_shows_through_transparent_pixels() {
        let mut pixels = [10, 20, 30, 255, 200, 200, 200, 0, 100, 100, 100, 128];
        over_color(&mut pixels, [0, 0, 255]);
        assert_eq!(&pixels[0..4], &[10, 20, 30, 255], "opaque stays");
        assert_eq!(
            &pixels[4..8],
            &[0, 0, 255, 255],
            "transparent becomes the colour"
        );
        assert_eq!(pixels[11], 255);
        assert!(pixels[10] > 100 && pixels[10] < 255, "half covered mixes");
    }
}
