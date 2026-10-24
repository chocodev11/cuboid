//! The I/O thread: one FIFO worker for file reads, atomic writes and the encoding that belongs to
//! them, so a backup and a save never write the same path at once (spec: arch 11.3).

use std::io;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::jobs::{JobCtx, JobError, JobHandle, run_isolated};

type Task = Box<dyn FnOnce() + Send>;

pub struct IoThread {
    tx: Option<Sender<Task>>,
    worker: Option<JoinHandle<()>>,
}

impl IoThread {
    pub fn spawn() -> io::Result<IoThread> {
        let (tx, rx) = mpsc::channel::<Task>();
        let worker = thread::Builder::new()
            .name("cuboid-io".to_owned())
            .spawn(move || {
                for task in rx {
                    task();
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
    use std::sync::mpsc::RecvTimeoutError;

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
