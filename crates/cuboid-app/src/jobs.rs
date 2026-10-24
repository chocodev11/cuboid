//! Job handles: progress, cancellation, panic isolation and results posted back to the UI thread
//! (spec: arch 11.3-11.4).

use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use thiserror::Error;

use crate::crash::payload_message;

/// Why a job did not produce a result.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum JobError {
    #[error("cancelled")]
    Cancelled,
    /// The file does not exist.
    #[error("the file does not exist")]
    NotFound,
    /// A message for the user.
    #[error("{0}")]
    Failed(String),
    #[error("internal error: {0}")]
    Panicked(String),
}

/// What a running job sees: the cancel flag and the progress it reports.
#[derive(Clone, Default)]
pub struct JobCtx {
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
}

/// Progress is stored in thousandths.
const FULL: u32 = 1000;

impl JobCtx {
    /// Fails with [`JobError::Cancelled`] once the UI asked to stop. Jobs call it between steps.
    pub fn check(&self) -> Result<(), JobError> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(JobError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Reports the finished part, 0 to 1.
    pub fn progress(&self, fraction: f32) {
        let thousandths = (fraction.clamp(0.0, 1.0) * FULL as f32).round();
        // The value is within 0..=1000.
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        self.progress.store(thousandths as u32, Ordering::Relaxed);
    }
}

/// The UI thread's side of a job.
pub struct JobHandle<T> {
    rx: Receiver<Result<T, JobError>>,
    ctx: JobCtx,
    finished: bool,
}

impl<T> JobHandle<T> {
    pub(crate) fn new(rx: Receiver<Result<T, JobError>>, ctx: JobCtx) -> Self {
        Self {
            rx,
            ctx,
            finished: false,
        }
    }

    /// The result when the job is done; never blocks. Gives `Some` once.
    pub fn poll(&mut self) -> Option<Result<T, JobError>> {
        if self.finished {
            return None;
        }
        let result = match self.rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                Err(JobError::Failed("the background thread stopped".to_owned()))
            }
        };
        self.finished = true;
        Some(result)
    }

    /// Asks the job to stop at its next check.
    pub fn cancel(&self) {
        self.ctx.cancel.store(true, Ordering::Relaxed);
    }

    /// The reported progress, 0 to 1.
    pub fn progress(&self) -> f32 {
        self.ctx.progress.load(Ordering::Relaxed) as f32 / FULL as f32
    }
}

/// Runs a job body with its panics turned into [`JobError::Panicked`]. A job cancelled before it
/// starts does not run.
pub(crate) fn run_isolated<T>(
    ctx: &JobCtx,
    body: impl FnOnce(&JobCtx) -> Result<T, JobError>,
) -> Result<T, JobError> {
    ctx.check()?;
    match panic::catch_unwind(AssertUnwindSafe(|| body(ctx))) {
        Ok(result) => result,
        Err(payload) => Err(JobError::Panicked(payload_message(payload.as_ref()))),
    }
}
