// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The worker threads that read and decode assets.

use std::num::NonZero;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SendError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

use nadir_core::log::warn;

/// Work for a worker: it reports its own result.
pub(crate) type Job = Box<dyn FnOnce() + Send>;

/// A few threads that take jobs, in the order they were given, from one
/// queue (`docs/01-architecture.md`, "Threading model").
pub(crate) struct Workers {
    /// `None` when no thread could start: jobs then run on the caller's.
    jobs: Option<Sender<Job>>,
    threads: Vec<JoinHandle<()>>,
    /// Set when the pool is dropped: the jobs still queued are dropped
    /// unrun, since nothing waits for them.
    stop: Arc<AtomicBool>,
}

impl Workers {
    /// How many workers a server starts: two less than the cores, from 1 to
    /// 4. That leaves the main thread a core of its own and one to spare,
    /// for the audio thread, the GPU driver's and the rest of the system,
    /// so that none of them takes the main thread's core while assets load
    /// (ADR-0030).
    pub(crate) fn count() -> usize {
        let cores = thread::available_parallelism().map_or(1, NonZero::get);
        cores.saturating_sub(2).clamp(1, 4)
    }

    /// A pool of `count` threads, or of as many as could start.
    pub(crate) fn new(count: usize) -> Self {
        let (sender, receiver) = mpsc::channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::with_capacity(count);
        for index in 0..count {
            let (receiver, stop) = (Arc::clone(&receiver), Arc::clone(&stop));
            let started = thread::Builder::new()
                .name(format!("nadir-assets-{index}"))
                .spawn(move || work(&receiver, &stop));
            match started {
                Ok(thread) => threads.push(thread),
                Err(error) => {
                    warn!("asset worker {index} could not start: {error}");
                    break;
                }
            }
        }
        if threads.is_empty() && count > 0 {
            warn!("no asset worker started: assets load on the thread that asks for them");
        }
        Self {
            jobs: (!threads.is_empty()).then_some(sender),
            threads,
            stop,
        }
    }

    /// Queues `job` for the next free worker. With no worker, it runs now.
    pub(crate) fn run(&self, job: Job) {
        match &self.jobs {
            Some(jobs) => {
                // Fails only if every worker has stopped, which they do only
                // when the pool is dropped.
                if let Err(SendError(job)) = jobs.send(job) {
                    job();
                }
            }
            None => job(),
        }
    }
}

/// A worker's loop: takes jobs until the queue closes.
fn work(jobs: &Mutex<Receiver<Job>>, stop: &AtomicBool) {
    loop {
        // The lock is held only while waiting for a job, never while one
        // runs. Nothing can panic while holding it, but a poisoned queue is
        // still a queue.
        let job = jobs.lock().unwrap_or_else(PoisonError::into_inner).recv();
        let Ok(job) = job else { return };
        if !stop.load(Ordering::Relaxed) {
            job();
        }
    }
}

impl Drop for Workers {
    /// Closes the queue and waits for the jobs that are running.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        drop(self.jobs.take());
        for thread in self.threads.drain(..) {
            // A worker's jobs catch their own panics, so it ends cleanly;
            // if one did not, there is nothing left to do for it.
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Condvar;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_server_starts_one_to_four_workers() {
        assert!((1..=4).contains(&Workers::count()));
    }

    #[test]
    fn jobs_run_on_the_workers_at_once() {
        let workers = Workers::new(3);
        assert_eq!(workers.threads.len(), 3);
        // Each job waits up to 10 s for the other two, so all three meet
        // only if they run at the same time. One run on this thread would
        // stop the others being queued until it gave up.
        let arrived = Arc::new((Mutex::new(0), Condvar::new()));
        let (done, finished) = mpsc::channel();
        let caller = thread::current().id();
        for _ in 0..3 {
            let (arrived, done) = (Arc::clone(&arrived), done.clone());
            workers.run(Box::new(move || {
                let (count, changed) = &*arrived;
                let mut count = count.lock().expect("the count");
                *count += 1;
                changed.notify_all();
                let (count, _) = changed
                    .wait_timeout_while(count, Duration::from_secs(10), |count| *count < 3)
                    .expect("the count");
                let met = *count == 3;
                done.send((thread::current().id() != caller, met))
                    .expect("the test waits");
            }));
        }
        for _ in 0..3 {
            let (elsewhere, met) = finished
                .recv_timeout(Duration::from_secs(30))
                .expect("every job finishes");
            assert!(elsewhere, "a job ran on the caller's thread");
            assert!(met, "the jobs did not run at the same time");
        }
    }

    #[test]
    fn with_no_worker_a_job_runs_at_once_on_the_callers_thread() {
        let workers = Workers::new(0);
        assert_eq!(workers.threads.len(), 0);
        let ran = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&ran);
        let caller = thread::current().id();
        workers.run(Box::new(move || {
            assert_eq!(thread::current().id(), caller);
            counter.fetch_add(1, Ordering::Relaxed);
        }));
        assert_eq!(ran.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn dropping_the_pool_waits_for_the_running_job() {
        let workers = Workers::new(1);
        let ran = Arc::new(AtomicUsize::new(0));
        let (started, running) = mpsc::channel();
        let (release, held) = mpsc::channel::<()>();
        {
            let ran = Arc::clone(&ran);
            workers.run(Box::new(move || {
                started.send(()).expect("the test waits");
                held.recv().expect("the test lets it go");
                ran.fetch_add(1, Ordering::Relaxed);
            }));
        }
        running
            .recv_timeout(Duration::from_secs(10))
            .expect("the job starts");
        let dropping = thread::spawn(move || drop(workers));
        // A drop that did not wait would be done long before this.
        thread::sleep(Duration::from_millis(50));
        assert!(!dropping.is_finished(), "the drop did not wait for the job");
        release.send(()).expect("the job waits");
        dropping.join().expect("the drop ends");
        assert_eq!(ran.load(Ordering::Relaxed), 1, "the drop ended first");
    }

    #[test]
    fn a_stopped_worker_drops_the_jobs_left_in_its_queue() {
        let (jobs, queue) = mpsc::channel::<Job>();
        let ran = Arc::new(AtomicUsize::new(0));
        for _ in 0..10 {
            let ran = Arc::clone(&ran);
            jobs.send(Box::new(move || {
                ran.fetch_add(1, Ordering::Relaxed);
            }))
            .expect("the queue is open");
        }
        drop(jobs);
        work(&Mutex::new(queue), &AtomicBool::new(true));
        assert_eq!(ran.load(Ordering::Relaxed), 0);
        assert_eq!(Arc::strong_count(&ran), 1, "the jobs were dropped");
    }
}
