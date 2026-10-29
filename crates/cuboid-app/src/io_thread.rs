//! The I/O thread: one FIFO worker for file reads, atomic writes and the encoding that belongs to
//! them, so a backup and a save never write the same path at once (spec: arch 11.3).
//!
//! Each task runs inside the background pool, so the parallel loops of an encode or a decode fill
//! that pool and never the global one, which the UI thread's loops (meshing, undo) use.

use std::io;
use std::num::NonZeroUsize;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rayon::{ThreadPool, ThreadPoolBuilder};

use crate::jobs::{JobCtx, JobError, JobHandle, run_isolated};

type Task = Box<dyn FnOnce() + Send>;

/// Threads of the background pool for `cores` logical cores: all but one, at least two and at
/// most eight. The work here (decoding and encoding a file, its chunk tables, library scans) is
/// CPU-bound per chunk, so a large voxel model opens and saves faster with more workers; the core
/// left over is the UI thread's, and the cap keeps a many-core machine from idling dozens of
/// threads. Never fewer than two: a worker takes the next injected task only after the one it
/// runs, so with a single worker a long library job would hold every save, autosave and open of
/// the document thread behind it.
fn pool_size(cores: usize) -> usize {
    cores.saturating_sub(1).clamp(2, 8)
}

fn background_threads() -> usize {
    pool_size(thread::available_parallelism().map_or(2, NonZeroUsize::get))
}

/// The pool of the background jobs of both I/O threads (documents and library), named
/// `cuboid-bg-N`. Built on first use.
fn background() -> io::Result<&'static ThreadPool> {
    static POOL: OnceLock<Result<ThreadPool, String>> = OnceLock::new();
    POOL.get_or_init(|| {
        ThreadPoolBuilder::new()
            .num_threads(background_threads())
            .thread_name(|index| format!("cuboid-bg-{index}"))
            .build()
            .map_err(|error| error.to_string())
    })
    .as_ref()
    .map_err(|message| io::Error::other(message.clone()))
}

pub struct IoThread {
    tx: Option<Sender<Task>>,
    worker: Option<JoinHandle<()>>,
}

impl IoThread {
    pub fn spawn() -> io::Result<IoThread> {
        let pool = background()?;
        let (tx, rx) = mpsc::channel::<Task>();
        let worker = thread::Builder::new()
            .name("cuboid-io".to_owned())
            .spawn(move || {
                for task in rx {
                    pool.install(task);
                }
            })?;
        Ok(IoThread {
            tx: Some(tx),
            worker: Some(worker),
        })
    }

    /// Queues `body` behind the jobs already queued. The handle gets the result, or the error if
    /// the body panicked or was cancelled before it started.
    pub fn submit<T: Send + 'static>(
        &self,
        body: impl FnOnce(&JobCtx) -> Result<T, JobError> + Send + 'static,
    ) -> JobHandle<T> {
        let (result_tx, result_rx) = mpsc::channel();
        let ctx = JobCtx::default();
        let handle = JobHandle::new(result_rx, ctx.clone());
        let task_tx = result_tx.clone();
        let task: Task = Box::new(move || {
            // The handle may be gone already; nobody wants the result then.
            let _ = task_tx.send(run_isolated(&ctx, body));
        });
        let sent = self.tx.as_ref().is_some_and(|tx| tx.send(task).is_ok());
        if !sent {
            let _ = result_tx.send(Err(JobError::Failed(
                "the file thread is not running".to_owned(),
            )));
        }
        handle
    }

    /// Lets the queued jobs finish and stops the thread, waiting at most `limit`. True when the
    /// thread ended in time.
    pub fn shutdown(&mut self, limit: Duration) -> bool {
        self.tx = None;
        let Some(worker) = self.worker.take() else {
            return true;
        };
        let deadline = Instant::now() + limit;
        while !worker.is_finished() {
            if Instant::now() >= deadline {
                log::warn!("the file thread did not finish within {limit:?}");
                return false;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let _ = worker.join();
        true
    }
}

impl Drop for IoThread {
    fn drop(&mut self) {
        self.shutdown(Duration::from_secs(3));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc::RecvTimeoutError;

    use rayon::prelude::*;

    use super::*;

    fn wait<T>(handle: &mut JobHandle<T>) -> Result<T, JobError> {
        let start = Instant::now();
        loop {
            if let Some(result) = handle.poll() {
                return result;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "job timed out");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn a_job_returns_its_result_and_progress() {
        let io = IoThread::spawn().unwrap();
        let mut job = io.submit(|ctx| {
            ctx.progress(0.5);
            Ok(42)
        });
        assert_eq!(wait(&mut job).unwrap(), 42);
        assert!((job.progress() - 0.5).abs() < 0.001);
        assert!(job.poll().is_none(), "the result comes once");
    }

    #[test]
    fn the_background_pool_is_all_cores_but_one_between_two_and_eight() {
        let table = [
            (1, 2),
            (2, 2),
            (3, 2),
            (4, 3),
            (6, 5),
            (9, 8),
            (16, 8),
            (64, 8),
        ];
        for (cores, threads) in table {
            assert_eq!(pool_size(cores), threads, "{cores} cores");
        }
        assert!((2..=8).contains(&background_threads()));
    }

    fn thread_name() -> String {
        thread::current().name().unwrap_or_default().to_owned()
    }

    #[test]
    fn a_job_and_its_parallel_loops_run_in_the_named_background_pool() {
        let io = IoThread::spawn().unwrap();
        let mut job = io.submit(|_| {
            let loop_names: Vec<String> = (0..256).into_par_iter().map(|_| thread_name()).collect();
            Ok((rayon::current_num_threads(), thread_name(), loop_names))
        });
        let (size, own, loop_names) = wait(&mut job).unwrap();
        assert_eq!(size, background_threads());
        assert!(own.starts_with("cuboid-bg-"), "{own}");
        assert!(loop_names.iter().all(|name| name.starts_with("cuboid-bg-")));
        // The thread that submits keeps the global pool.
        let outside: Vec<String> = (0..256).into_par_iter().map(|_| thread_name()).collect();
        assert!(outside.iter().all(|name| !name.starts_with("cuboid-bg-")));
    }

    #[test]
    fn a_parallel_loop_outside_does_not_wait_for_a_busy_background_pool() {
        let io = IoThread::spawn().unwrap();
        // One item for each worker of the global pool, which is what a shared pool would give the
        // job; each item holds its worker until the gate opens.
        let items = rayon::current_num_threads();
        let held = items.min(background_threads());
        let open = Arc::new(AtomicBool::new(false));
        let started = Arc::new(AtomicUsize::new(0));
        let (gate, count) = (Arc::clone(&open), Arc::clone(&started));
        let mut busy = io.submit(move |_| {
            (0..items).into_par_iter().with_max_len(1).for_each(|_| {
                count.fetch_add(1, Ordering::SeqCst);
                let start = Instant::now();
                while !gate.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(10) {
                    thread::sleep(Duration::from_millis(1));
                }
            });
            Ok(())
        });
        let start = Instant::now();
        while started.load(Ordering::SeqCst) < held {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "workers never filled"
            );
            thread::sleep(Duration::from_millis(1));
        }
        // Every background worker is held. The UI thread's loop is injected into the global pool.
        let (tx, rx) = mpsc::channel();
        let ui = thread::spawn(move || tx.send((0..1000_u64).into_par_iter().sum::<u64>()));
        let sum = rx.recv_timeout(Duration::from_secs(5));
        open.store(true, Ordering::SeqCst);
        wait(&mut busy).unwrap();
        ui.join().unwrap().unwrap();
        assert_eq!(sum, Ok(499_500));
    }

    #[test]
    fn a_long_job_of_one_io_thread_does_not_hold_the_jobs_of_another() {
        assert!(background_threads() >= 2);
        let documents = IoThread::spawn().unwrap();
        let library = IoThread::spawn().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        // Holds its worker until the gate sender is dropped.
        let mut long = library.submit(move |_| {
            started_tx.send(()).unwrap();
            let _ = gate_rx.recv_timeout(Duration::from_secs(10));
            Ok(())
        });
        started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let mut quick = documents.submit(|_| Ok(7));
        let start = Instant::now();
        let answer = loop {
            if let Some(result) = quick.poll() {
                break Some(result);
            }
            if start.elapsed() > Duration::from_secs(5) {
                break None;
            }
            thread::sleep(Duration::from_millis(2));
        };
        drop(gate_tx);
        wait(&mut long).unwrap();
        assert_eq!(answer, Some(Ok(7)));
    }

    #[test]
    fn jobs_run_in_submission_order() {
        let io = IoThread::spawn().unwrap();
        let (tx, rx) = mpsc::channel();
        let mut handles = Vec::new();
        for index in 0..20 {
            let tx = tx.clone();
            handles.push(io.submit(move |_| {
                tx.send(index).unwrap();
                Ok(())
            }));
        }
        for handle in &mut handles {
            wait(handle).unwrap();
        }
        let order: Vec<i32> = rx.try_iter().collect();
        assert_eq!(order, (0..20).collect::<Vec<_>>());
    }

    #[test]
    fn a_running_job_stops_at_its_cancel_check() {
        let io = IoThread::spawn().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let mut job = io.submit(move |ctx| -> Result<(), JobError> {
            started_tx.send(()).unwrap();
            loop {
                ctx.check()?;
                thread::sleep(Duration::from_millis(1));
            }
        });
        started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        job.cancel();
        assert_eq!(wait(&mut job), Err(JobError::Cancelled));
    }

    #[test]
    fn a_job_cancelled_while_queued_never_runs() {
        let io = IoThread::spawn().unwrap();
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let mut blocker = io.submit(move |_| {
            let waited = gate_rx.recv_timeout(Duration::from_secs(10));
            assert!(!matches!(waited, Err(RecvTimeoutError::Timeout)));
            Ok(())
        });
        let (ran_tx, ran_rx) = mpsc::channel();
        let mut queued = io.submit(move |_| {
            ran_tx.send(()).unwrap();
            Ok(())
        });
        queued.cancel();
        gate_tx.send(()).unwrap();
        wait(&mut blocker).unwrap();
        assert_eq!(wait(&mut queued), Err(JobError::Cancelled));
        assert!(ran_rx.try_recv().is_err());
    }

    #[test]
    fn a_failing_job_reports_its_message() {
        let io = IoThread::spawn().unwrap();
        let mut job = io.submit(|_| Err::<(), _>(JobError::Failed("disk full".to_owned())));
        assert_eq!(
            wait(&mut job),
            Err(JobError::Failed("disk full".to_owned()))
        );
    }

    #[test]
    fn a_panicking_job_becomes_an_error_and_the_thread_goes_on() {
        let io = IoThread::spawn().unwrap();
        let mut bad = io.submit(|_| -> Result<(), JobError> { panic!("job exploded") });
        let error = wait(&mut bad).unwrap_err();
        assert_eq!(error, JobError::Panicked("job exploded".to_owned()));
        let mut good = io.submit(|_| Ok(1));
        assert_eq!(wait(&mut good).unwrap(), 1);
    }

    #[test]
    fn submitting_after_shutdown_fails_without_blocking() {
        let mut io = IoThread::spawn().unwrap();
        assert!(io.shutdown(Duration::from_secs(5)));
        let mut job = io.submit(|_| Ok(()));
        assert!(matches!(wait(&mut job), Err(JobError::Failed(_))));
    }

    #[test]
    fn shutdown_waits_for_queued_work() {
        let mut io = IoThread::spawn().unwrap();
        let (tx, rx) = mpsc::channel();
        let _job = io.submit(move |_| {
            thread::sleep(Duration::from_millis(50));
            tx.send(()).unwrap();
            Ok(())
        });
        assert!(io.shutdown(Duration::from_secs(5)));
        assert!(rx.try_recv().is_ok());
    }
}
