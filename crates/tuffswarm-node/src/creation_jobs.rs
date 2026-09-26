//! In-memory Creation Marketplace job queue (worker node ↔ desktop poller).

use libp2p::request_response::ResponseChannel;
use std::collections::{HashMap, VecDeque};
use std::time::Instant;
use tuffbox_core::creation_marketplace::{CreationJob, CreationResult};

pub struct PendingCreationEntry {
    pub job: CreationJob,
    pub channel: ResponseChannel<CreationResult>,
    pub enqueued_at: Instant,
}

pub struct PendingCreationJobs {
    max_jobs: usize,
    queue: VecDeque<PendingCreationEntry>,
    inflight: HashMap<String, PendingCreationEntry>,
}

impl PendingCreationJobs {
    pub fn new(max_jobs: u32) -> Self {
        Self {
            max_jobs: max_jobs.max(1) as usize,
            queue: VecDeque::new(),
            inflight: HashMap::new(),
        }
    }

    pub fn active_count(&self) -> usize {
        self.queue.len() + self.inflight.len()
    }

    pub fn is_busy(&self) -> bool {
        self.active_count() >= self.max_jobs
    }

    pub fn enqueue(&mut self, entry: PendingCreationEntry) -> Result<(), PendingCreationEntry> {
        if self.is_busy() {
            return Err(entry);
        }
        self.queue.push_back(entry);
        Ok(())
    }

    pub fn take_pending(&mut self) -> Option<CreationJob> {
        let entry = self.queue.pop_front()?;
        let job = entry.job.clone();
        self.inflight.insert(entry.job.job_id.clone(), entry);
        Some(job)
    }

    pub fn complete(&mut self, job_id: &str) -> Option<PendingCreationEntry> {
        self.inflight.remove(job_id)
    }

    pub fn expire_overdue(&mut self) -> Vec<PendingCreationEntry> {
        let now = Instant::now();
        let mut expired = Vec::new();

        let mut kept = VecDeque::new();
        while let Some(entry) = self.queue.pop_front() {
            let deadline = std::time::Duration::from_millis(entry.job.deadline_ms.max(1_000));
            if now.duration_since(entry.enqueued_at) > deadline {
                expired.push(entry);
            } else {
                kept.push_back(entry);
            }
        }
        self.queue = kept;

        let overdue_ids: Vec<String> = self
            .inflight
            .iter()
            .filter(|(_, e)| {
                let deadline = std::time::Duration::from_millis(e.job.deadline_ms.max(1_000));
                now.duration_since(e.enqueued_at) > deadline
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in overdue_ids {
            if let Some(entry) = self.inflight.remove(&id) {
                expired.push(entry);
            }
        }
        expired
    }
}
