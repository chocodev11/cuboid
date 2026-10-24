//! Offscreen targets for the PNG render (spec: app 15; arch 13.8).

use cuboid_image::check_size;

use crate::composite::{depth_attachment, draw_list};
use crate::contract::{PngJob, ReadbackId, RenderError, UploadBudget};
use crate::gpu::{PNG_SLOT, Renderer};
use crate::look::CLEAR_PNG;
use crate::readback::Pending;
use crate::targets::{COLOR, DEPTH};
use crate::util::guarded;

impl Renderer {
    /// Draws the job into an offscreen target of `W * aa x H * aa` cleared to transparent black
    /// and starts the read-back; the targets are released after the read-back. The result
    /// arrives through [`Renderer::poll_readbacks`], which also reports a render that could not
    /// start. The extract of the job is applied to the renderer's buffers first, without a
    /// budget; an empty extract leaves them alone.
    ///
    /// spec: app 15, arch 13.8
    pub fn start_png_render(&mut self, enc: &mut wgpu::CommandEncoder, job: &PngJob) -> ReadbackId {
        let id = self.next_id();
        let pending = self.record_png(enc, job, id).unwrap_or_else(|err| {
            log::error!("PNG render failed: {err}");
            Pending::failed(id, err)
        });
        self.readbacks.push(pending);
        id
    }

    fn record_png(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        job: &PngJob,
        id: ReadbackId,
    ) -> Result<Pending, RenderError> {
        if self.device_lost() {
            return Err(RenderError::DeviceLost);
        }
        let factor = if job.antialias { 2 } else { 1 };
        let (w, h) = (
            job.camera.size.0.saturating_mul(factor),
            job.camera.size.1.saturating_mul(factor),
        );
        check_size(w, h).map_err(|err| RenderError::Readback(err.to_string()))?;
        if w > self.caps.max_texture_size || h > self.caps.max_texture_size {
            return Err(RenderError::Unsupported(
                "a render larger than the texture limit",
            ));
        }
        let extract = &job.extract;
        if extract.instance_count > 0
            || !(extract.instances.is_empty()
                && extract.atlas.is_empty()
                && extract.chunks.is_empty()
                && extract.meshes.is_empty())
        {
            let queue = self.queue.clone();
            let everything = UploadBudget {
                bytes: usize::MAX,
                millis: f32::INFINITY,
            };
            self.sync(&queue, extract, everything);
        }
        self.write_view(PNG_SLOT, &job.camera);

        let device = &self.device;
        let (Some(gpu), true) = (&self.gpu, w > 0 && h > 0) else {
            return Err(RenderError::Unsupported("a renderer without a device"));
        };
        let Some(pipes) = &gpu.pipes else {
            return Err(RenderError::Gpu("the pipelines are missing".to_owned()));
        };
        let samples = pipes.samples;
        let extent = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let padded = wgpu::util::align_to(w * 4, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let (target, buffer, msaa, depth) = guarded(device, "PNG render target", || {
            let texture = |label, format, samples, usage| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let target = texture(
                "png_target",
                COLOR,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            );
            let msaa = (samples > 1).then(|| {
                texture(
                    "png_msaa",
                    COLOR,
                    samples,
                    wgpu::TextureUsages::RENDER_ATTACHMENT,
                )
            });
            let depth = texture(
                "png_depth",
                DEPTH,
                samples,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("png_readback"),
                size: u64::from(padded) * u64::from(h),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            (target, buffer, msaa, depth)
        })?;

        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let msaa_view = msaa.map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let (color, resolve) = match &msaa_view {
                Some(msaa) => (msaa, Some(&target_view)),
                None => (&target_view, None),
            };
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("png_scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color,
                    resolve_target: resolve,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR_PNG),
                        store: if resolve.is_some() {
                            wgpu::StoreOp::Discard
                        } else {
                            wgpu::StoreOp::Store
                        },
                    },
                })],
                depth_stencil_attachment: Some(depth_attachment(&depth_view)),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            draw_list(gpu, pipes, &mut pass, &job.list, PNG_SLOT, None);
        }
        enc.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            extent,
        );
        Ok(Pending::recorded(id, buffer, (w, h), padded, factor))
    }
}
