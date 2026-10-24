//! Cast helpers and the guarded resource creation of wgpu error scopes (spec: arch 13.9).

use crate::contract::RenderError;

/// A length as `u32`, saturating (buffers are far below 4 GiB of records).
pub(crate) fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// A length as `u64`.
pub(crate) fn to_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// A non-negative float rounded to the nearest `u32`, clamped to the `u32` range.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to the u32 range first"
)]
pub(crate) fn round_u32(v: f32) -> u32 {
    v.round().clamp(0.0, 4.0e9) as u32
}

/// Runs `create` with an out-of-memory and a validation error scope around it and turns what the
/// scopes caught into an error, so a failed buffer, texture or pipeline never aborts the process.
pub(crate) fn guarded<T>(
    device: &wgpu::Device,
    what: &'static str,
    create: impl FnOnce() -> T,
) -> Result<T, RenderError> {
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let value = create();
    // The pop futures of the native backends are ready at once.
    let invalid = pollster::block_on(validation.pop());
    let exhausted = pollster::block_on(oom.pop());
    if exhausted.is_some() {
        return Err(RenderError::OutOfMemory(what));
    }
    match invalid {
        Some(err) => Err(RenderError::Gpu(format!("{what}: {err}"))),
        None => Ok(value),
    }
}
