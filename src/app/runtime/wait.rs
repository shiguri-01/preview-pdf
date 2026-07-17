use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, error::TryRecvError};
use tokio::time;

use crate::event::DomainEvent;
use crate::extension::ExtensionWorkerEvent;
use crate::presenter::ImagePresenter;
use crate::render::worker::RenderWorker;

use super::RuntimeEvent;

const EXTENSION_WORKER_BATCH_LIMIT: usize = 128;

pub(in crate::app) struct RuntimeEventSources<'a> {
    pub(in crate::app) event_rx: &'a mut UnboundedReceiver<DomainEvent>,
    pub(in crate::app) extension_worker_rx: &'a mut UnboundedReceiver<ExtensionWorkerEvent>,
    pub(in crate::app) render_worker: &'a mut RenderWorker,
    pub(in crate::app) presenter: &'a mut dyn ImagePresenter,
    pub(in crate::app) prefetch_tick: &'a mut time::Interval,
    pub(in crate::app) redraw_tick: &'a mut time::Interval,
}

pub(in crate::app) async fn wait_next_event(
    sources: RuntimeEventSources<'_>,
    wait_for_pending_redraw: bool,
    wake_timeout: Duration,
) -> RuntimeEvent {
    let RuntimeEventSources {
        event_rx,
        extension_worker_rx,
        render_worker,
        presenter,
        prefetch_tick,
        redraw_tick,
    } = sources;
    let presenter_pending = presenter.has_pending_work();
    let extension_worker_available =
        !extension_worker_rx.is_closed() || !extension_worker_rx.is_empty();
    tokio::select! {
        biased;
        maybe_event = event_rx.recv() => {
            match maybe_event {
                Some(event) => RuntimeEvent::Event(event),
                None => RuntimeEvent::Closed,
            }
        },
        maybe_render = render_worker.recv_result() => {
            match maybe_render {
                Some(result) => RuntimeEvent::Event(DomainEvent::RenderComplete(result)),
                None => RuntimeEvent::Closed,
            }
        },
        maybe_extension = extension_worker_rx.recv(), if extension_worker_available => {
            match maybe_extension {
                Some(event) => RuntimeEvent::Event(DomainEvent::ExtensionWorker(drain_extension_worker_batch(extension_worker_rx, event))),
                None => RuntimeEvent::Event(DomainEvent::Wake),
            }
        },
        maybe_presenter = presenter.recv_background_event(), if presenter_pending => {
            match maybe_presenter {
                Some(event) => RuntimeEvent::Event(DomainEvent::EncodeComplete(event)),
                None => RuntimeEvent::Event(DomainEvent::Wake),
            }
        },
        _ = prefetch_tick.tick() => {
            RuntimeEvent::Event(DomainEvent::PrefetchTick)
        },
        _ = redraw_tick.tick(), if wait_for_pending_redraw => {
            RuntimeEvent::Event(DomainEvent::RedrawTick)
        },
        _ = time::sleep(wake_timeout) => {
            RuntimeEvent::Event(DomainEvent::Wake)
        }
    }
}

fn drain_extension_worker_batch(
    extension_worker_rx: &mut UnboundedReceiver<ExtensionWorkerEvent>,
    first: ExtensionWorkerEvent,
) -> Vec<ExtensionWorkerEvent> {
    let mut events = Vec::with_capacity(EXTENSION_WORKER_BATCH_LIMIT.min(8));
    events.push(first);
    while events.len() < EXTENSION_WORKER_BATCH_LIMIT {
        match extension_worker_rx.try_recv() {
            Ok(event) => events.push(event),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::runtime::Builder;
    use tokio::sync::mpsc::unbounded_channel;

    use super::{RuntimeEventSources, wait_next_event};
    use crate::backend::test_support::{build_pdf, unique_temp_path};
    use crate::backend::{PdfDoc, SharedPdfBackend};
    use crate::command::{Command, CommandInvocationSource, CommandRequest};
    use crate::event::DomainEvent;
    use crate::extension::ExtensionWorkerEvent;
    use crate::presenter::{
        ImagePresenter, PresenterBackgroundEvent, PresenterCaps, PresenterFeedback,
        PresenterRenderOutcome, PresenterRenderSlot, PresenterRuntimeInfo, PresenterSlot,
    };
    use crate::render::worker::RenderWorker;
    use crate::search::engine::{SearchEpoch, SearchEvent, SearchSnapshot};

    use super::super::RuntimeEvent;

    #[derive(Default)]
    struct StubPresenter {
        events: VecDeque<Option<PresenterBackgroundEvent>>,
    }

    impl StubPresenter {
        fn with_events(events: impl IntoIterator<Item = Option<PresenterBackgroundEvent>>) -> Self {
            Self {
                events: events.into_iter().collect(),
            }
        }
    }

    impl ImagePresenter for StubPresenter {
        fn prepare_slots(&mut self, _slots: &[PresenterSlot<'_>]) -> crate::error::AppResult<()> {
            Ok(())
        }

        fn render_slots(
            &mut self,
            _frame: &mut ratatui::Frame<'_>,
            _slots: &[PresenterRenderSlot],
        ) -> crate::error::AppResult<PresenterRenderOutcome> {
            Ok(PresenterRenderOutcome {
                drew_image: false,
                feedback: PresenterFeedback::None,
                used_stale_fallback: false,
                slots: Vec::new(),
            })
        }

        fn capabilities(&self) -> PresenterCaps {
            PresenterCaps {
                backend_name: "stub",
                supports_l2_cache: false,
                cell_px: None,
                preferred_max_render_scale: 1.0,
            }
        }

        fn runtime_info(&self) -> PresenterRuntimeInfo {
            PresenterRuntimeInfo::default()
        }

        fn has_pending_work(&self) -> bool {
            !self.events.is_empty()
        }

        fn recv_background_event<'a>(
            &'a mut self,
        ) -> Pin<Box<dyn Future<Output = Option<PresenterBackgroundEvent>> + 'a>> {
            let event = self.events.pop_front().flatten();
            Box::pin(async move { event })
        }
    }

    fn idle_render_worker() -> RenderWorker {
        let file = unique_temp_path(".pdf");
        fs::write(&file, build_pdf(&["page"])).expect("test pdf should be created");
        let doc = PdfDoc::open(&file).expect("pdf should open");
        fs::remove_file(&file).expect("test pdf should be removed");
        let shared: SharedPdfBackend = Arc::new(doc);
        RenderWorker::spawn(shared, 1)
    }

    #[test]
    fn wait_next_event_maps_presenter_event_then_eof_to_wake() {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let mut render_worker = idle_render_worker();
        let mut presenter = StubPresenter::with_events([
            Some(PresenterBackgroundEvent::EncodeComplete {
                redraw_requested: true,
            }),
            None,
        ]);
        let (_tx, mut event_rx) = unbounded_channel();
        let (_extension_tx, mut extension_worker_rx) = unbounded_channel();
        runtime.block_on(async {
            let mut prefetch_tick = tokio::time::interval(Duration::from_secs(60));
            let mut redraw_tick = tokio::time::interval(Duration::from_secs(60));

            let first = wait_next_event(
                RuntimeEventSources {
                    event_rx: &mut event_rx,
                    extension_worker_rx: &mut extension_worker_rx,
                    render_worker: &mut render_worker,
                    presenter: &mut presenter,
                    prefetch_tick: &mut prefetch_tick,
                    redraw_tick: &mut redraw_tick,
                },
                false,
                Duration::from_secs(60),
            )
            .await;
            assert!(matches!(
                first,
                RuntimeEvent::Event(DomainEvent::EncodeComplete(
                    PresenterBackgroundEvent::EncodeComplete {
                        redraw_requested: true
                    }
                ))
            ));

            let second = wait_next_event(
                RuntimeEventSources {
                    event_rx: &mut event_rx,
                    extension_worker_rx: &mut extension_worker_rx,
                    render_worker: &mut render_worker,
                    presenter: &mut presenter,
                    prefetch_tick: &mut prefetch_tick,
                    redraw_tick: &mut redraw_tick,
                },
                false,
                Duration::from_secs(60),
            )
            .await;
            assert!(matches!(second, RuntimeEvent::Event(DomainEvent::Wake)));
        });
    }

    #[test]
    fn wait_next_event_drains_buffered_extension_events_after_sender_closes() {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let mut render_worker = idle_render_worker();
        let mut presenter = StubPresenter::default();
        let (_event_tx, mut event_rx) = unbounded_channel();
        let (extension_tx, mut extension_worker_rx) = unbounded_channel();
        let event = SearchEvent::Snapshot(SearchSnapshot {
            epoch: SearchEpoch(1),
            generation: 1,
            scanned_pages: 1,
            total_pages: 2,
            hit_pages: 1,
            done: false,
        });
        extension_tx
            .send(ExtensionWorkerEvent::Search(event.clone()))
            .expect("buffered worker event should send");
        drop(extension_tx);

        runtime.block_on(async {
            let mut prefetch_tick = tokio::time::interval(Duration::from_secs(60));
            let mut redraw_tick = tokio::time::interval(Duration::from_secs(60));

            let waited = wait_next_event(
                RuntimeEventSources {
                    event_rx: &mut event_rx,
                    extension_worker_rx: &mut extension_worker_rx,
                    render_worker: &mut render_worker,
                    presenter: &mut presenter,
                    prefetch_tick: &mut prefetch_tick,
                    redraw_tick: &mut redraw_tick,
                },
                false,
                Duration::from_secs(60),
            )
            .await;

            match waited {
                RuntimeEvent::Event(DomainEvent::ExtensionWorker(events)) => {
                    assert_eq!(events, vec![ExtensionWorkerEvent::Search(event)]);
                }
                _ => panic!("expected buffered extension worker event"),
            }
        });
    }

    #[test]
    fn wait_next_event_does_not_let_closed_extension_channel_mask_domain_events() {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let mut render_worker = idle_render_worker();
        let mut presenter = StubPresenter::default();
        let (event_tx, mut event_rx) = unbounded_channel();
        let (extension_tx, mut extension_worker_rx) = unbounded_channel();
        drop(extension_tx);
        let request = CommandRequest::new(Command::OpenHelp, CommandInvocationSource::Binding);
        event_tx
            .send(DomainEvent::Command(request.clone()))
            .expect("runtime event should send");

        runtime.block_on(async {
            let mut prefetch_tick = tokio::time::interval(Duration::from_secs(60));
            let mut redraw_tick = tokio::time::interval(Duration::from_secs(60));

            let waited = wait_next_event(
                RuntimeEventSources {
                    event_rx: &mut event_rx,
                    extension_worker_rx: &mut extension_worker_rx,
                    render_worker: &mut render_worker,
                    presenter: &mut presenter,
                    prefetch_tick: &mut prefetch_tick,
                    redraw_tick: &mut redraw_tick,
                },
                false,
                Duration::from_secs(60),
            )
            .await;

            match waited {
                RuntimeEvent::Event(DomainEvent::Command(received)) => {
                    assert_eq!(received, request);
                }
                _ => panic!("expected runtime command event"),
            }
        });
    }
}
