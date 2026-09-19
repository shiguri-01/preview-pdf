use std::path::{Path, PathBuf};

use crossterm::event::EventStream;
use futures_util::StreamExt;
use notify_debouncer_full::notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;
use tokio::time::{self, Duration};

use crate::backend::open_default_backend;
use crate::error::{AppError, AppResult};
use crate::event::DocumentReloadReason;
use crate::event::DocumentReloadRequest;
use crate::event::DocumentReloadResult;
use crate::event::DomainEvent;

pub(crate) struct EventBusRuntime {
    tasks: Vec<JoinHandle<()>>,
    file_watch: Option<Debouncer<RecommendedWatcher, RecommendedCache>>,
}

impl EventBusRuntime {
    pub(crate) fn spawn_headless() -> (
        UnboundedSender<DomainEvent>,
        UnboundedReceiver<DomainEvent>,
        Self,
    ) {
        let (tx, rx) = unbounded_channel();
        (
            tx,
            rx,
            Self {
                tasks: Vec::new(),
                file_watch: None,
            },
        )
    }

    pub(crate) fn start_input(&mut self, tx: UnboundedSender<DomainEvent>) {
        self.push_task(spawn_input_task(tx));
    }

    pub(crate) fn start_file_watch(
        &mut self,
        path: PathBuf,
        settle_delay: Duration,
        tx: UnboundedSender<DomainEvent>,
    ) -> AppResult<()> {
        let path = path.canonicalize().map_err(|source| {
            AppError::io_with_context(source, "resolving PDF path for file watching")
        })?;
        let parent = path
            .parent()
            .expect("canonical PDF path has a parent")
            .to_path_buf();
        let mut watcher = new_debouncer(settle_delay, None, move |result| {
            forward_file_watch_events(&path, result, &tx);
        })
        .map_err(|source| {
            AppError::io_with_context(std::io::Error::other(source), "starting PDF file watcher")
        })?;
        // Editors and PDF generators often save by replacing the file. Watching
        // the directory keeps observing the path after that replacement.
        watcher
            .watch(&parent, RecursiveMode::NonRecursive)
            .map_err(|source| {
                AppError::io_with_context(std::io::Error::other(source), "watching PDF directory")
            })?;
        self.file_watch = Some(watcher);
        Ok(())
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
        self.file_watch.take();
        for task in self.tasks.drain(..) {
            task.abort();
        }
    }

    fn push_task(&mut self, task: JoinHandle<()>) {
        self.tasks.retain(|task| !task.is_finished());
        self.tasks.push(task);
    }
}

impl Drop for EventBusRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn spawn_input_task(tx: UnboundedSender<DomainEvent>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut input_stream = EventStream::new();
        while let Some(event) = input_stream.next().await {
            let domain_event = match event {
                Ok(event) => DomainEvent::Input(event),
                Err(err) => DomainEvent::InputError(err.to_string()),
            };
            if tx.send(domain_event).is_err() {
                return;
            }
        }
    })
}

fn forward_file_watch_events(
    path: &Path,
    result: DebounceEventResult,
    tx: &UnboundedSender<DomainEvent>,
) {
    match result {
        Ok(events) => {
            let changed = events.iter().any(|event| {
                event.need_rescan()
                    || (matches!(
                        event.kind,
                        EventKind::Any
                            | EventKind::Create(_)
                            | EventKind::Modify(_)
                            | EventKind::Remove(_)
                    ) && event.paths.iter().any(|changed_path| changed_path == path))
            });
            if changed {
                let _ = tx.send(DomainEvent::ReloadDocument(DocumentReloadRequest::new(
                    DocumentReloadReason::FileChanged,
                )));
            }
        }
        Err(errors) => {
            let message = errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ");
            let _ = tx.send(DomainEvent::FileWatchError(message));
        }
    }
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
    use std::path::Path;
    use std::time::Duration;
    use std::time::Instant;

    use notify_debouncer_full::DebouncedEvent;
    use notify_debouncer_full::notify::event::{
        AccessKind, CreateKind, DataChange, Flag, ModifyKind, RenameMode,
    };
    use notify_debouncer_full::notify::{Error, Event, EventKind};
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
    use tokio::time;

    use crate::backend::test_support::{build_pdf, unique_temp_path};
    use crate::event::{DocumentReloadReason, DocumentReloadRequest, DomainEvent};

    use super::{EventBusRuntime, forward_file_watch_events};

    #[test]
    fn spawn_creates_runtime_without_tasks() {
        let (_tx, _rx, mut runtime) = EventBusRuntime::spawn_headless();
        assert!(runtime.tasks.is_empty());
        runtime.shutdown();
    }

    #[test]
    fn spawned_runtime_can_start_input() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should initialize");
        runtime.block_on(async {
            let (tx, _rx, mut runtime) = EventBusRuntime::spawn_headless();
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
    fn file_watch_ignores_reads_and_unrelated_files() {
        let path = Path::new("/documents/current.pdf");
        let (tx, mut rx) = unbounded_channel();
        let events = vec![
            Event::new(EventKind::Access(AccessKind::Read)).add_path(path.to_path_buf()),
            Event::new(EventKind::Create(CreateKind::File)).add_path("/documents/other.pdf".into()),
        ];
        forward_file_watch_events(path, Ok(debounced(events)), &tx);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn file_watch_batch_requests_one_reload_for_replacement_and_writes() {
        let path = Path::new("/documents/current.pdf");
        let (tx, mut rx) = unbounded_channel();
        let events = vec![
            Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                .add_path("/documents/replacement.pdf".into())
                .add_path(path.to_path_buf()),
            Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
                .add_path(path.to_path_buf()),
        ];
        forward_file_watch_events(path, Ok(debounced(events)), &tx);
        assert!(
            matches!(rx.try_recv(), Ok(DomainEvent::ReloadDocument(request))
            if request.reason == DocumentReloadReason::FileChanged && !request.retry)
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn file_watch_requests_reload_when_native_events_were_lost() {
        let path = Path::new("/documents/current.pdf");
        let (tx, mut rx) = unbounded_channel();
        let events = vec![Event::new(EventKind::Other).set_flag(Flag::Rescan)];
        forward_file_watch_events(path, Ok(debounced(events)), &tx);
        assert!(matches!(rx.try_recv(), Ok(DomainEvent::ReloadDocument(_))));
    }

    #[test]
    fn file_watch_errors_reach_the_event_loop() {
        let (tx, mut rx) = unbounded_channel();
        forward_file_watch_events(
            Path::new("/documents/current.pdf"),
            Err(vec![Error::generic("watch failed")]),
            &tx,
        );
        assert!(
            matches!(rx.try_recv(), Ok(DomainEvent::FileWatchError(message))
            if message.contains("watch failed"))
        );
    }

    fn debounced(events: Vec<Event>) -> Vec<DebouncedEvent> {
        events
            .into_iter()
            .map(|event| DebouncedEvent::new(event, Instant::now()))
            .collect()
    }

    async fn expect_file_reload(rx: &mut UnboundedReceiver<DomainEvent>) {
        let event = time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("native watcher should deliver its event")
            .expect("watcher channel should stay open");
        assert!(matches!(event, DomainEvent::ReloadDocument(request)
            if request.reason == DocumentReloadReason::FileChanged && !request.retry));
    }

    #[test]
    fn file_watch_observes_writes_deletion_and_recreation() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should initialize");
        let directory = unique_temp_path("watch_reload");
        fs::create_dir(&directory).expect("test directory should be created");
        let file = directory.join("current.pdf");
        fs::write(&file, build_pdf(&["before"])).expect("test pdf should be written");

        runtime.block_on(async {
            let (tx, mut rx, mut event_runtime) = EventBusRuntime::spawn_headless();
            event_runtime
                .start_file_watch(file.clone(), Duration::from_millis(40), tx)
                .expect("watcher should start");
            fs::write(&file, build_pdf(&["after"])).expect("test pdf should change");
            expect_file_reload(&mut rx).await;

            fs::remove_file(&file).expect("test pdf should be removed");
            expect_file_reload(&mut rx).await;
            fs::write(&file, build_pdf(&["recreated"])).expect("test pdf should be recreated");
            expect_file_reload(&mut rx).await;

            #[cfg(unix)]
            {
                let replacement = directory.join("replacement.pdf");
                fs::write(&replacement, build_pdf(&["replaced"]))
                    .expect("replacement should be written");
                fs::rename(&replacement, &file).expect("test pdf should be atomically replaced");
                expect_file_reload(&mut rx).await;
                fs::write(&file, build_pdf(&["after replacement"]))
                    .expect("replacement should remain watched");
                expect_file_reload(&mut rx).await;
            }

            event_runtime.shutdown();
            assert!(event_runtime.file_watch.is_none());
        });

        fs::remove_file(&file).expect("test file should be removed");
        fs::remove_dir(&directory).expect("test directory should be removed");
    }
}
