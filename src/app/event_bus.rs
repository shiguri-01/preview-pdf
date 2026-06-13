use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crossterm::event::EventStream;
use futures_util::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;
use tokio::time::{self, Duration, Instant};

use crate::backend::open_default_backend;
use crate::event::DocumentReloadReason;
use crate::event::DocumentReloadRequest;
use crate::event::DocumentReloadResult;
use crate::event::DomainEvent;

pub(crate) struct EventBusRuntime {
    tasks: Vec<JoinHandle<()>>,
}

impl EventBusRuntime {
    pub(crate) fn spawn_interactive() -> (
        UnboundedSender<DomainEvent>,
        UnboundedReceiver<DomainEvent>,
        Self,
    ) {
        let (tx, rx) = unbounded_channel();
        (tx, rx, Self { tasks: Vec::new() })
    }

    pub(crate) fn spawn_headless() -> (
        UnboundedSender<DomainEvent>,
        UnboundedReceiver<DomainEvent>,
        Self,
    ) {
        let (tx, rx) = unbounded_channel();
        let tasks = Vec::new();
        (tx, rx, Self { tasks })
    }

    pub(crate) fn start_input(&mut self, tx: UnboundedSender<DomainEvent>) {
        self.push_task(spawn_input_task(tx));
    }

    pub(crate) fn start_file_watch(
        &mut self,
        path: PathBuf,
        poll_interval: Duration,
        settle_delay: Duration,
        tx: UnboundedSender<DomainEvent>,
    ) {
        self.push_task(spawn_file_watch_task(path, poll_interval, settle_delay, tx));
    }

    pub(crate) fn start_document_reload(
        &mut self,
        path: PathBuf,
        request: DocumentReloadRequest,
        tx: UnboundedSender<DomainEvent>,
    ) {
        self.push_task(spawn_document_reload_task(path, request, tx));
    }

    pub(crate) fn start_delayed_document_reload(
        &mut self,
        request: DocumentReloadRequest,
        delay: Duration,
        tx: UnboundedSender<DomainEvent>,
    ) {
        self.push_task(spawn_delayed_document_reload_task(request, delay, tx));
    }

    pub(crate) fn shutdown(&mut self) {
        for task in self.tasks.drain(..) {
            task.abort();
        }
    }

    fn push_task(&mut self, task: JoinHandle<()>) {
        self.tasks.retain(|task| !task.is_finished());
        self.tasks.push(task);
    }
}

fn spawn_input_task(tx: UnboundedSender<DomainEvent>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut input_stream = EventStream::new();
        while let Some(event) = input_stream.next().await {
            let loop_event = match event {
                Ok(event) => DomainEvent::Input(event),
                Err(err) => DomainEvent::InputError(err.to_string()),
            };
            if tx.send(loop_event).is_err() {
                return;
            }
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSignature {
    exists: bool,
    len: Option<u64>,
    modified: Option<SystemTime>,
}

fn file_signature(path: &Path) -> FileSignature {
    let Ok(metadata) = std::fs::metadata(path) else {
        return FileSignature {
            exists: false,
            len: None,
            modified: None,
        };
    };
    FileSignature {
        exists: true,
        len: Some(metadata.len()),
        modified: metadata.modified().ok(),
    }
}

fn spawn_file_watch_task(
    path: PathBuf,
    poll_interval: Duration,
    settle_delay: Duration,
    tx: UnboundedSender<DomainEvent>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut last_seen = file_signature(&path);
        let mut pending_since: Option<Instant> = None;
        let mut interval = time::interval(poll_interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            let current = file_signature(&path);
            if current != last_seen {
                last_seen = current;
                pending_since = Some(Instant::now());
                continue;
            }

            let Some(started_at) = pending_since else {
                continue;
            };
            if started_at.elapsed() < settle_delay {
                continue;
            }
            pending_since = None;
            if tx
                .send(DomainEvent::ReloadDocument(DocumentReloadRequest::new(
                    DocumentReloadReason::FileChanged,
                )))
                .is_err()
            {
                return;
            }
        }
    })
}

fn spawn_document_reload_task(
    path: PathBuf,
    request: DocumentReloadRequest,
    tx: UnboundedSender<DomainEvent>,
) -> JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        let result = open_default_backend(&path).map_err(|err| err.to_string());
        let _ = tx.send(DomainEvent::DocumentReloaded(DocumentReloadResult {
            reason: request.reason,
            generation: request.generation,
            result,
        }));
    })
}

fn spawn_delayed_document_reload_task(
    request: DocumentReloadRequest,
    delay: Duration,
    tx: UnboundedSender<DomainEvent>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        time::sleep(delay).await;
        let _ = tx.send(DomainEvent::ReloadDocument(request));
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use tokio::time;

    use crate::backend::test_support::{build_pdf, unique_temp_path};
    use crate::event::{DocumentReloadReason, DocumentReloadRequest, DomainEvent};

    use super::EventBusRuntime;

    #[test]
    fn spawn_headless_creates_runtime_without_tasks() {
        let (_tx, _rx, mut runtime) = EventBusRuntime::spawn_headless();
        assert!(runtime.tasks.is_empty());
        runtime.shutdown();
    }

    #[test]
    fn spawn_interactive_creates_runtime_with_tasks() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should initialize");
        runtime.block_on(async {
            let (tx, _rx, mut runtime) = EventBusRuntime::spawn_interactive();
            runtime.start_input(tx);
            runtime.shutdown();
        });
    }

    #[test]
    fn starting_task_prunes_finished_handles() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should initialize");
        runtime.block_on(async {
            let (tx, mut rx, mut event_runtime) = EventBusRuntime::spawn_headless();
            event_runtime.start_delayed_document_reload(
                DocumentReloadRequest::retry(DocumentReloadReason::FileChanged, 1),
                Duration::ZERO,
                tx.clone(),
            );
            let _ = rx.recv().await.expect("first delayed reload should emit");

            event_runtime.start_delayed_document_reload(
                DocumentReloadRequest::retry(DocumentReloadReason::FileChanged, 1),
                Duration::from_secs(60),
                tx,
            );

            assert_eq!(event_runtime.tasks.len(), 1);
            event_runtime.shutdown();
        });
    }

    #[test]
    fn file_watch_emits_reload_after_changed_file_settles() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should initialize");
        let file = unique_temp_path("watch_reload.pdf");
        fs::write(&file, build_pdf(&["before"])).expect("test pdf should be written");

        runtime.block_on(async {
            let (tx, mut rx, mut event_runtime) = EventBusRuntime::spawn_headless();
            event_runtime.start_file_watch(
                file.clone(),
                Duration::from_millis(20),
                Duration::from_millis(40),
                tx,
            );
            time::sleep(Duration::from_millis(30)).await;
            fs::write(&file, build_pdf(&["after"])).expect("test pdf should change");

            let event = time::timeout(Duration::from_secs(1), rx.recv())
                .await
                .expect("watcher should emit before timeout")
                .expect("watcher channel should stay open");
            assert!(matches!(
                event,
                DomainEvent::ReloadDocument(request)
                    if request.reason == DocumentReloadReason::FileChanged && !request.retry
            ));
            event_runtime.shutdown();
        });

        fs::remove_file(&file).expect("test file should be removed");
    }
}
