//! Non-blocking GPU read-back with `map_async` and polling (spec: arch 11.4, 13.8).
//!
//! A PNG render copies its target into a buffer inside the caller's encoder. `poll_readbacks`
//! maps the buffer on its first call after the render started, polls the device without
//! blocking, copies the mapped bytes out and hands them to a worker thread that removes the row
//! padding, flips the rows and downscales.

use std::sync::mpsc::{Receiver, TryRecvError, channel};

use cuboid_image::{RgbaBuf, check_size};

use crate::contract::{ReadbackId, RenderError};
use crate::gpu::Renderer;

/// Where a read-back is.
#[derive(Debug)]
enum Stage {
    /// The copy is recorded; the buffer is mapped by the next poll.
    Recorded(wgpu::Buffer),
    /// `map_async` is pending.
    Mapping(wgpu::Buffer, Receiver<Result<(), wgpu::BufferAsyncError>>),
    /// A worker converts the pixels.
    Working(Receiver<Result<RgbaBuf, RenderError>>),
    /// The render failed before or during the read-back.
    Failed(Option<RenderError>),
}

/// A PNG render waiting for its pixels.
#[derive(Debug)]
pub(crate) struct Pending {
    id: ReadbackId,
    /// Size of the rendered target in pixels.
    size: (u32, u32),
    /// Bytes per row of the buffer, padded to the copy alignment.
    padded: u32,
    /// 2 when the render is downscaled 2 x 2.
    factor: u32,
    stage: Stage,
}

impl Pending {
    /// A render whose copy is recorded in the caller's encoder.
    pub fn recorded(
        id: ReadbackId,
        buffer: wgpu::Buffer,
        size: (u32, u32),
        padded: u32,
        factor: u32,
    ) -> Pending {
        Pending {
            id,
            size,
            padded,
            factor,
            stage: Stage::Recorded(buffer),
        }
    }

    /// A render that could not start.
    pub fn failed(id: ReadbackId, error: RenderError) -> Pending {
        Pending {
            id,
            size: (0, 0),
            padded: 0,
            factor: 1,
            stage: Stage::Failed(Some(error)),
        }
    }

    /// Turns the render into a failure; the next poll reports it.
    pub fn fail(&mut self, error: RenderError) {
        self.stage = Stage::Failed(Some(error));
    }

    /// Advances the read-back without blocking. `Some` when it is over.
    fn step(&mut self) -> Option<Result<RgbaBuf, RenderError>> {
        let stage = std::mem::replace(&mut self.stage, Stage::Failed(None));
        match stage {
            Stage::Recorded(buffer) => {
                let (tx, rx) = channel();
                buffer
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        // The receiver lives as long as the read-back.
                        let _ = tx.send(result);
                    });
                self.stage = Stage::Mapping(buffer, rx);
                None
            }
            Stage::Mapping(buffer, rx) => match rx.try_recv() {
                Err(TryRecvError::Empty) => {
                    self.stage = Stage::Mapping(buffer, rx);
                    None
                }
                Err(TryRecvError::Disconnected) | Ok(Err(_)) => Some(Err(RenderError::Readback(
                    "the buffer could not be mapped".to_owned(),
                ))),
                Ok(Ok(())) => {
                    let data = buffer
                        .slice(..)
                        .get_mapped_range()
                        .map(|view| view.to_vec());
                    buffer.unmap();
                    match data {
                        Ok(data) => self.convert(data),
                        Err(err) => Some(Err(RenderError::Readback(err.to_string()))),
                    }
                }
            },
            Stage::Working(rx) => match rx.try_recv() {
                Err(TryRecvError::Empty) => {
                    self.stage = Stage::Working(rx);
                    None
                }
                Err(TryRecvError::Disconnected) => Some(Err(RenderError::Readback(
                    "the conversion stopped".to_owned(),
                ))),
                Ok(result) => Some(result),
            },
            Stage::Failed(error) => Some(Err(error.unwrap_or(RenderError::DeviceLost))),
        }
    }

    /// Hands the mapped bytes to a worker thread, or converts them here when no thread can start.
    fn convert(&mut self, data: Vec<u8>) -> Option<Result<RgbaBuf, RenderError>> {
        let (size, padded, factor) = (self.size, self.padded, self.factor);
        let (tx, rx) = channel();
        let work = move || {
            let _ = tx.send(finish(&data, size, padded, factor));
        };
        match std::thread::Builder::new()
            .name("png-readback".to_owned())
            .spawn(work)
        {
            Ok(_) => {
                self.stage = Stage::Working(rx);
                None
            }
            Err(err) => Some(Err(RenderError::Readback(format!(
                "no worker thread: {err}"
            )))),
        }
    }
}

/// Removes the row padding, flips the rows to bottom-to-top and applies the 2 x 2 downscale.
fn finish(data: &[u8], size: (u32, u32), padded: u32, factor: u32) -> Result<RgbaBuf, RenderError> {
    check_size(size.0, size.1).map_err(|err| RenderError::Readback(err.to_string()))?;
    let (w, h) = (size.0 as usize, size.1 as usize);
    let (row, stride) = (w * 4, padded as usize);
    if data.len() < stride * h || stride < row {
        return Err(RenderError::Readback("the read-back is short".to_owned()));
    }
    let mut full = RgbaBuf::new(size.0, size.1);
    for (y, dst) in full.pixels.chunks_exact_mut(row).enumerate() {
        let src = (h - 1 - y) * stride;
        dst.copy_from_slice(&data[src..src + row]);
    }
    Ok(if factor == 2 { downscale(&full) } else { full })
}

/// The 2 x 2 box downscale of app 15: output pixel `(x, y)` from the bottom left averages the
/// source columns `2x - 1`, `2x` and rows `2y - 1`, `2y` of straight RGBA, using the samples
/// that exist. The last source column and row are never read.
pub(crate) fn downscale(src: &RgbaBuf) -> RgbaBuf {
    let (w, h) = (src.w / 2, src.h / 2);
    let mut out = RgbaBuf::new(w.max(1), h.max(1));
    let stride = src.w as usize * 4;
    for y in 0..h as usize {
        for x in 0..w as usize {
            let mut sum = [0u32; 4];
            let mut n = 0;
            for sy in (2 * y).saturating_sub(1)..=2 * y {
                for sx in (2 * x).saturating_sub(1)..=2 * x {
                    let at = sy * stride + sx * 4;
                    for (s, c) in sum.iter_mut().zip(&src.pixels[at..at + 4]) {
                        *s += u32::from(*c);
                    }
                    n += 1;
                }
            }
            let at = (y * w as usize + x) * 4;
            for (d, s) in out.pixels[at..at + 4].iter_mut().zip(sum) {
                *d = u8::try_from((s + n / 2) / n).unwrap_or(u8::MAX);
            }
        }
    }
    out
}

impl Renderer {
    /// Polls the pending read-backs without blocking and returns the finished renders. The
    /// pixels are the render target's, rows bottom to top as everywhere in the image crate,
    /// already downscaled when the job asked for anti-aliasing. A render that failed reports its
    /// error. Call it after the encoder of `start_png_render` was submitted, once per frame. The
    /// device is the one the renderer holds (the argument is kept for compatibility).
    ///
    /// spec: arch 11.4 (rule 5), arch 13.8
    pub fn poll_readbacks(
        &mut self,
        _device: &wgpu::Device,
    ) -> Vec<(ReadbackId, Result<RgbaBuf, RenderError>)> {
        if let Err(err) = self.device.poll(wgpu::PollType::Poll) {
            log::error!("cannot poll the device: {err}");
        }
        let mut finished = Vec::new();
        let mut waiting = Vec::with_capacity(self.readbacks.len());
        for mut pending in self.readbacks.drain(..) {
            match pending.step() {
                Some(result) => finished.push((pending.id, result)),
                None => waiting.push(pending),
            }
        }
        self.readbacks = waiting;
        finished
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::cast_possible_truncation, reason = "test arithmetic")]
    use super::*;

    fn gradient(w: u32, h: u32) -> RgbaBuf {
        let mut buf = RgbaBuf::new(w, h);
        for y in 0..h {
            for x in 0..w {
                buf.set(x as i32, y as i32, [(x * 10) as u8, (y * 10) as u8, 0, 255]);
            }
        }
        buf
    }

    #[test]
    fn the_downscale_averages_the_blocks_shifted_toward_the_origin() {
        let src = gradient(4, 4);
        let out = downscale(&src);
        assert_eq!((out.w, out.h), (2, 2));
        // Output (0, 0) has only the source column 0 and row 0: nothing to average.
        assert_eq!(out.get(0, 0), Some([0, 0, 0, 255]));
        // Output (1, 1) averages columns 1, 2 and rows 1, 2: 15 and 15.
        assert_eq!(out.get(1, 1), Some([15, 15, 0, 255]));
        // Output (1, 0) averages columns 1, 2 and rows 0 (only): 15 and 0.
        assert_eq!(out.get(1, 0), Some([15, 0, 0, 255]));
    }

    #[test]
    fn rows_are_unpadded_and_flipped() {
        // Two rows of one pixel, padded to 8 bytes; the first row is the top one.
        let data = [1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8, 0, 0, 0, 0];
        let out = finish(&data, (1, 2), 8, 1).unwrap();
        assert_eq!(out.pixels, [5, 6, 7, 8, 1, 2, 3, 4]);
    }
}
