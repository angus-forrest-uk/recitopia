use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};

use thiserror::Error;
use tokio::sync::Notify;

use crate::model::{
    CookbookImportProgress, ImportJobState, ImportPipelineStage, IntroductionPageDiagnostic,
};

#[derive(Clone, Debug)]
pub struct CancellationSignal {
    inner: Arc<CancellationInner>,
}

#[derive(Debug)]
struct CancellationInner {
    canceled: AtomicBool,
    notify: Notify,
}

impl CancellationSignal {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CancellationInner {
                canceled: AtomicBool::new(false),
                notify: Notify::new(),
            }),
        }
    }

    pub fn cancel(&self) {
        if !self.inner.canceled.swap(true, Ordering::AcqRel) {
            self.inner.notify.notify_waiters();
        }
    }

    #[must_use]
    pub fn is_canceled(&self) -> bool {
        self.inner.canceled.load(Ordering::Acquire)
    }

    pub async fn cancelled(&self) {
        loop {
            let notified = self.inner.notify.notified();
            if self.is_canceled() {
                return;
            }
            notified.await;
        }
    }
}

impl Default for CancellationSignal {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
struct JobEntry {
    progress: CookbookImportProgress,
    cancellation: CancellationSignal,
}

#[derive(Clone, Debug)]
pub struct BeginJob {
    pub started: bool,
    pub progress: CookbookImportProgress,
    pub cancellation: CancellationSignal,
}

#[derive(Debug, Default)]
pub struct JobRegistry {
    entries: Mutex<HashMap<String, JobEntry>>,
    diagnostics: Mutex<HashMap<String, IntroductionPageDiagnostic>>,
}

impl JobRegistry {
    /// Starts a job unless the same id is already running.
    ///
    /// # Errors
    ///
    /// Returns [`JobRegistryError`] if the registry mutex was poisoned.
    pub fn begin(&self, id: &str) -> Result<BeginJob, JobRegistryError> {
        let mut entries = self.entries()?;
        if let Some(existing) = entries.get(id) {
            if existing.progress.state == ImportJobState::Running {
                return Ok(BeginJob {
                    started: false,
                    progress: existing.progress.clone(),
                    cancellation: existing.cancellation.clone(),
                });
            }
        }

        let progress = CookbookImportProgress::queued(id);
        let cancellation = CancellationSignal::new();
        entries.insert(
            id.to_owned(),
            JobEntry {
                progress: progress.clone(),
                cancellation: cancellation.clone(),
            },
        );
        Ok(BeginJob {
            started: true,
            progress,
            cancellation,
        })
    }

    /// Returns an in-memory progress snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`JobRegistryError`] if the registry mutex was poisoned.
    pub fn get(&self, id: &str) -> Result<Option<CookbookImportProgress>, JobRegistryError> {
        Ok(self.entries()?.get(id).map(|entry| entry.progress.clone()))
    }

    /// Applies an ordered progress mutation and returns its resulting snapshot.
    /// Updates cannot overwrite a canceled terminal state.
    ///
    /// # Errors
    ///
    /// Returns [`JobRegistryError`] if the registry mutex was poisoned.
    pub fn update<F>(
        &self,
        id: &str,
        update: F,
    ) -> Result<Option<CookbookImportProgress>, JobRegistryError>
    where
        F: FnOnce(&mut CookbookImportProgress),
    {
        let mut entries = self.entries()?;
        let Some(entry) = entries.get_mut(id) else {
            return Ok(None);
        };
        if entry.progress.state != ImportJobState::Canceled {
            update(&mut entry.progress);
        }
        Ok(Some(entry.progress.clone()))
    }

    /// Requests cooperative cancellation and returns the frozen snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`JobRegistryError`] if the registry mutex was poisoned.
    pub fn cancel(&self, id: &str) -> Result<Option<CookbookImportProgress>, JobRegistryError> {
        let mut entries = self.entries()?;
        let Some(entry) = entries.get_mut(id) else {
            return Ok(None);
        };
        if entry.progress.state == ImportJobState::Running {
            entry.cancellation.cancel();
            entry.progress.state = ImportJobState::Canceled;
            entry.progress.stage = ImportPipelineStage::Canceled;
            entry.progress.message = "Cancellation requested.".to_owned();
            entry.progress.current_section_title = None;
            entry.progress.error_message = None;
        }
        Ok(Some(entry.progress.clone()))
    }

    /// Stores an introduction diagnostic result for its result endpoint.
    ///
    /// # Errors
    ///
    /// Returns [`JobRegistryError`] if the result mutex was poisoned.
    pub fn put_diagnostic(
        &self,
        result: IntroductionPageDiagnostic,
    ) -> Result<(), JobRegistryError> {
        self.diagnostics()?.insert(result.job_id.clone(), result);
        Ok(())
    }

    /// Returns a completed introduction diagnostic result.
    ///
    /// # Errors
    ///
    /// Returns [`JobRegistryError`] if the result mutex was poisoned.
    pub fn diagnostic(
        &self,
        id: &str,
    ) -> Result<Option<IntroductionPageDiagnostic>, JobRegistryError> {
        Ok(self.diagnostics()?.get(id).cloned())
    }

    fn entries(&self) -> Result<MutexGuard<'_, HashMap<String, JobEntry>>, JobRegistryError> {
        self.entries
            .lock()
            .map_err(|_| JobRegistryError::LockPoisoned)
    }

    fn diagnostics(
        &self,
    ) -> Result<MutexGuard<'_, HashMap<String, IntroductionPageDiagnostic>>, JobRegistryError> {
        self.diagnostics
            .lock()
            .map_err(|_| JobRegistryError::LockPoisoned)
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum JobRegistryError {
    #[error("job registry lock is poisoned")]
    LockPoisoned,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_is_terminal_and_a_running_job_is_not_started_twice() {
        let registry = JobRegistry::default();
        let first = registry.begin("import-1").unwrap();
        let duplicate = registry.begin("import-1").unwrap();

        assert!(first.started);
        assert!(!duplicate.started);
        assert!(Arc::ptr_eq(
            &first.cancellation.inner,
            &duplicate.cancellation.inner
        ));

        let canceled = registry.cancel("import-1").unwrap().unwrap();
        assert_eq!(canceled.state, ImportJobState::Canceled);
        assert!(first.cancellation.is_canceled());

        let frozen = registry
            .update("import-1", |progress| {
                progress.state = ImportJobState::Complete;
                progress.stage = ImportPipelineStage::Complete;
                progress.message = "should not win".to_owned();
            })
            .unwrap()
            .unwrap();
        assert_eq!(frozen.state, ImportJobState::Canceled);
        assert_eq!(frozen.message, "Cancellation requested.");
    }
}
