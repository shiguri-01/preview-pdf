use ratatui::layout::Rect;
use ratatui_image::FilterType;
use ratatui_image::Resize;
use ratatui_image::ResizeEncodeRender;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use tokio::runtime::{Builder, Handle, Runtime};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, error::TryRecvError, unbounded_channel,
};
use tokio::task::JoinHandle;

use crate::backend::RgbaFrame;
use crate::render::prefetch::{PrefetchQueue, PrefetchQueueConfig, QueueTaskMeta};
use crate::work::WorkClass;

use super::image_ops::{create_protocol_with_picker, resize_frame_for_area};
use super::l2_cache::TerminalFrameKey;

pub(crate) const ENCODE_RESIZE_FILTER: FilterType = FilterType::Nearest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EncodeLaneKind {
    Current,
    Background,
}

pub(crate) enum EncodeWorkerRequest {
    Encode {
        key: TerminalFrameKey,
        picker: Picker,
        frame: RgbaFrame,
        area: Rect,
        allow_upscale: bool,
        class: WorkClass,
        generation: u64,
        enqueued_at: std::time::Instant,
    },
    Shutdown,
}

pub(crate) struct EncodeWorkerTask {
    pub(crate) key: TerminalFrameKey,
    pub(crate) picker: Picker,
    pub(crate) frame: RgbaFrame,
    pub(crate) area: Rect,
    pub(crate) allow_upscale: bool,
    pub(crate) enqueued_at: std::time::Instant,
}

pub(crate) struct EncodeWorkerResult {
    pub(crate) lane: EncodeLaneKind,
    pub(crate) event: EncodeWorkerEvent,
}

pub(crate) enum EncodeWorkerEvent {
    Completed {
        key: TerminalFrameKey,
        protocol: Option<Box<StatefulProtocol>>,
        queue_wait: std::time::Duration,
        elapsed: std::time::Duration,
        succeeded: bool,
    },
    CanceledStale {
        key: TerminalFrameKey,
    },
    QueueState {
        depth: usize,
        in_flight: usize,
    },
}

pub(crate) struct EncodeWorkerRuntime {
    _owned: Option<Runtime>,
    handle: Handle,
}

impl EncodeWorkerRuntime {
    pub(crate) fn new() -> Self {
        if let Ok(handle) = Handle::try_current() {
            return Self {
                _owned: None,
                handle,
            };
        }

        let runtime = Builder::new_multi_thread()
            .enable_all()
            .thread_name("pvf-encode")
            .build()
            .expect("encode runtime should initialize");
        let handle = runtime.handle().clone();
        Self {
            _owned: Some(runtime),
            handle,
        }
    }

    fn spawn_blocking<F>(&self, task: F) -> JoinHandle<()>
    where
        F: FnOnce() + Send + 'static,
    {
        self.handle.spawn_blocking(task)
    }
}

pub(crate) fn send_encode_request(
    request_tx: &Option<UnboundedSender<EncodeWorkerRequest>>,
    request: EncodeWorkerRequest,
) -> Result<(), Box<EncodeWorkerRequest>> {
    let Some(request_tx) = request_tx.as_ref() else {
        return Err(Box::new(request));
    };
    request_tx.send(request).map_err(|err| Box::new(err.0))
}

pub(crate) fn spawn_encode_worker(
    runtime: &EncodeWorkerRuntime,
    lane: EncodeLaneKind,
) -> (
    UnboundedSender<EncodeWorkerRequest>,
    UnboundedReceiver<EncodeWorkerResult>,
    JoinHandle<()>,
) {
    let (request_tx, request_rx) = unbounded_channel();
    let (result_tx, result_rx) = unbounded_channel();
    let worker = runtime.spawn_blocking(move || encode_worker_main(lane, request_rx, result_tx));
    (request_tx, result_rx, worker)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn enqueue_encode_request(
    lane: EncodeLaneKind,
    request: EncodeWorkerRequest,
    queue: &mut PrefetchQueue<TerminalFrameKey, EncodeWorkerTask>,
) -> bool {
    match request {
        EncodeWorkerRequest::Encode {
            key,
            picker,
            frame,
            area,
            allow_upscale,
            class,
            generation,
            enqueued_at,
        } => {
            let _ = cancel_stale_tasks_with_keys(lane, queue, generation);
            if lane == EncodeLaneKind::Current
                && retain_current_lane_same_key(queue, key, generation)
            {
                return true;
            }

            let task = EncodeWorkerTask {
                key,
                picker,
                frame,
                area,
                allow_upscale,
                enqueued_at,
            };
            let meta = QueueTaskMeta {
                key,
                class,
                generation,
            };
            let _ = queue.push(task, meta);
            true
        }
        EncodeWorkerRequest::Shutdown => false,
    }
}

fn cancel_stale_tasks_with_keys(
    lane: EncodeLaneKind,
    queue: &mut PrefetchQueue<TerminalFrameKey, EncodeWorkerTask>,
    generation: u64,
) -> Vec<TerminalFrameKey> {
    let mut removed = Vec::new();
    let _ = queue.retain(|_, meta| {
        let keep = match lane {
            EncodeLaneKind::Current => meta.generation >= generation,
            EncodeLaneKind::Background => {
                meta.generation >= generation || meta.class.kept_on_background_stale_generation()
            }
        };
        if !keep {
            removed.push(meta.key);
        }
        keep
    });
    removed
}

fn retain_current_lane_same_key(
    queue: &mut PrefetchQueue<TerminalFrameKey, EncodeWorkerTask>,
    key: TerminalFrameKey,
    generation: u64,
) -> bool {
    let mut newer_duplicate_queued = false;
    let _ = queue.retain(|_, meta| {
        if meta.key != key {
            return true;
        }
        if meta.generation > generation {
            newer_duplicate_queued = true;
            return true;
        }
        false
    });
    newer_duplicate_queued
}

fn enqueue_with_notifications(
    lane: EncodeLaneKind,
    request: EncodeWorkerRequest,
    queue: &mut PrefetchQueue<TerminalFrameKey, EncodeWorkerTask>,
    result_tx: &UnboundedSender<EncodeWorkerResult>,
) -> bool {
    match request {
        EncodeWorkerRequest::Encode {
            key,
            picker,
            frame,
            area,
            allow_upscale,
            class,
            generation,
            enqueued_at,
        } => {
            let canceled = cancel_stale_tasks_with_keys(lane, queue, generation);
            let canceled_count = canceled.len();
            for canceled_key in canceled {
                let _ = result_tx.send(EncodeWorkerResult {
                    lane,
                    event: EncodeWorkerEvent::CanceledStale { key: canceled_key },
                });
            }
            if canceled_count > 0 {
                let _ = result_tx.send(EncodeWorkerResult {
                    lane,
                    event: EncodeWorkerEvent::QueueState {
                        depth: queue.len(),
                        in_flight: 0,
                    },
                });
            }
            if lane == EncodeLaneKind::Current
                && retain_current_lane_same_key(queue, key, generation)
            {
                return true;
            }

            let task = EncodeWorkerTask {
                key,
                picker,
                frame,
                area,
                allow_upscale,
                enqueued_at,
            };
            let meta = QueueTaskMeta {
                key,
                class,
                generation,
            };
            let _ = queue.push(task, meta);
            let _ = result_tx.send(EncodeWorkerResult {
                lane,
                event: EncodeWorkerEvent::QueueState {
                    depth: queue.len(),
                    in_flight: 0,
                },
            });
            true
        }
        EncodeWorkerRequest::Shutdown => false,
    }
}

pub(crate) fn pop_next_encode_task(
    queue: &mut PrefetchQueue<TerminalFrameKey, EncodeWorkerTask>,
) -> Option<EncodeWorkerTask> {
    queue.pop_next()
}

fn encode_worker_main(
    lane: EncodeLaneKind,
    mut request_rx: UnboundedReceiver<EncodeWorkerRequest>,
    result_tx: UnboundedSender<EncodeWorkerResult>,
) {
    let mut queue = PrefetchQueue::new(PrefetchQueueConfig::default());

    loop {
        if queue.is_empty() {
            let request = match request_rx.blocking_recv() {
                Some(request) => request,
                None => break,
            };
            if !enqueue_with_notifications(lane, request, &mut queue, &result_tx) {
                break;
            }
        }

        loop {
            match request_rx.try_recv() {
                Ok(request) => {
                    if !enqueue_with_notifications(lane, request, &mut queue, &result_tx) {
                        return;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }

        let Some(task) = pop_next_encode_task(&mut queue) else {
            continue;
        };
        let _ = result_tx.send(EncodeWorkerResult {
            lane,
            event: EncodeWorkerEvent::QueueState {
                depth: queue.len(),
                in_flight: 1,
            },
        });

        let started = std::time::Instant::now();
        let frame = match resize_frame_for_area(
            task.frame,
            task.area,
            task.picker.font_size(),
            task.allow_upscale,
        ) {
            Ok(frame) => frame,
            Err(_) => {
                let _ = result_tx.send(EncodeWorkerResult {
                    lane,
                    event: EncodeWorkerEvent::Completed {
                        key: task.key,
                        protocol: None,
                        queue_wait: started.saturating_duration_since(task.enqueued_at),
                        elapsed: started.elapsed(),
                        succeeded: false,
                    },
                });
                let _ = result_tx.send(EncodeWorkerResult {
                    lane,
                    event: EncodeWorkerEvent::QueueState {
                        depth: queue.len(),
                        in_flight: 0,
                    },
                });
                continue;
            }
        };
        let mut protocol = match create_protocol_with_picker(&task.picker, frame) {
            Ok(protocol) => protocol,
            Err(_) => {
                let _ = result_tx.send(EncodeWorkerResult {
                    lane,
                    event: EncodeWorkerEvent::Completed {
                        key: task.key,
                        protocol: None,
                        queue_wait: started.saturating_duration_since(task.enqueued_at),
                        elapsed: started.elapsed(),
                        succeeded: false,
                    },
                });
                let _ = result_tx.send(EncodeWorkerResult {
                    lane,
                    event: EncodeWorkerEvent::QueueState {
                        depth: queue.len(),
                        in_flight: 0,
                    },
                });
                continue;
            }
        };
        protocol.resize_encode(&Resize::Fit(Some(ENCODE_RESIZE_FILTER)), task.area);
        let succeeded = protocol
            .last_encoding_result()
            .map(|result| result.is_ok())
            .unwrap_or(true);

        let _ = result_tx.send(EncodeWorkerResult {
            lane,
            event: EncodeWorkerEvent::Completed {
                key: task.key,
                protocol: if succeeded {
                    Some(Box::new(protocol))
                } else {
                    None
                },
                queue_wait: started.saturating_duration_since(task.enqueued_at),
                elapsed: started.elapsed(),
                succeeded,
            },
        });
        let _ = result_tx.send(EncodeWorkerResult {
            lane,
            event: EncodeWorkerEvent::QueueState {
                depth: queue.len(),
                in_flight: 0,
            },
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use ratatui::layout::Rect;
    use ratatui_image::picker::Picker;
    use tokio::sync::mpsc::unbounded_channel;

    use crate::backend::RgbaFrame;
    use crate::presenter::l2_cache::TerminalFrameKey;
    use crate::presenter::{PanOffset, Viewport};
    use crate::render::cache::RenderedPageKey;
    use crate::render::prefetch::{PrefetchQueue, PrefetchQueueConfig};
    use crate::work::WorkClass;

    use super::{
        EncodeLaneKind, EncodeWorkerEvent, EncodeWorkerRequest, EncodeWorkerTask,
        enqueue_encode_request, enqueue_with_notifications,
    };

    fn frame() -> RgbaFrame {
        RgbaFrame {
            width: 4,
            height: 4,
            pixels: vec![0xaa; 4 * 4 * 4].into(),
        }
    }

    fn key(page: usize) -> TerminalFrameKey {
        TerminalFrameKey {
            rendered_page: RenderedPageKey::new(1, page, 1.0),
            viewport: Viewport {
                x: 0,
                y: 0,
                width: 10,
                height: 6,
            },
            pan: PanOffset::default(),
            overlay_stamp: 0,
        }
    }

    #[test]
    fn enqueue_with_notifications_emits_canceled_stale_events() {
        let mut queue: PrefetchQueue<TerminalFrameKey, EncodeWorkerTask> =
            PrefetchQueue::new(PrefetchQueueConfig::default());
        let picker = Picker::halfblocks();
        let stale_key = key(1);
        let fresh_key = key(2);
        let area = Rect::new(0, 0, 10, 6);

        assert!(enqueue_encode_request(
            EncodeLaneKind::Background,
            EncodeWorkerRequest::Encode {
                key: stale_key,
                picker: picker.clone(),
                frame: frame(),
                area,
                allow_upscale: false,
                class: WorkClass::DirectionalLead,
                generation: 1,
                enqueued_at: Instant::now(),
            },
            &mut queue
        ));

        let (tx, mut rx) = unbounded_channel();
        assert!(enqueue_with_notifications(
            EncodeLaneKind::Background,
            EncodeWorkerRequest::Encode {
                key: fresh_key,
                picker,
                frame: frame(),
                area,
                allow_upscale: false,
                class: WorkClass::CriticalCurrent,
                generation: 2,
                enqueued_at: Instant::now(),
            },
            &mut queue,
            &tx
        ));

        let mut saw_canceled = false;
        while let Ok(event) = rx.try_recv() {
            if matches!(event.event, EncodeWorkerEvent::CanceledStale { key } if key == stale_key) {
                saw_canceled = true;
                break;
            }
        }
        assert!(saw_canceled, "canceled-stale event should be emitted");
    }

    #[test]
    fn current_lane_drops_older_generations() {
        let mut queue: PrefetchQueue<TerminalFrameKey, EncodeWorkerTask> =
            PrefetchQueue::new(PrefetchQueueConfig::default());
        let picker = Picker::halfblocks();
        let area = Rect::new(0, 0, 10, 6);
        let stale_key = key(1);
        let fresh_key = key(2);

        assert!(enqueue_encode_request(
            EncodeLaneKind::Current,
            EncodeWorkerRequest::Encode {
                key: stale_key,
                picker: picker.clone(),
                frame: frame(),
                area,
                allow_upscale: false,
                class: WorkClass::CriticalCurrent,
                generation: 1,
                enqueued_at: Instant::now(),
            },
            &mut queue
        ));

        assert!(enqueue_encode_request(
            EncodeLaneKind::Current,
            EncodeWorkerRequest::Encode {
                key: fresh_key,
                picker,
                frame: frame(),
                area,
                allow_upscale: false,
                class: WorkClass::CriticalCurrent,
                generation: 2,
                enqueued_at: Instant::now(),
            },
            &mut queue
        ));

        let first = super::pop_next_encode_task(&mut queue).expect("fresh current should remain");
        assert_eq!(first.key, fresh_key);
        assert!(super::pop_next_encode_task(&mut queue).is_none());
    }

    #[test]
    fn current_lane_keeps_newer_same_key_when_older_request_arrives_late() {
        let mut queue: PrefetchQueue<TerminalFrameKey, EncodeWorkerTask> =
            PrefetchQueue::new(PrefetchQueueConfig::default());
        let picker = Picker::halfblocks();
        let same_key = key(1);
        let newer_area = Rect::new(0, 0, 10, 6);
        let older_area = Rect::new(0, 0, 6, 4);

        assert!(enqueue_encode_request(
            EncodeLaneKind::Current,
            EncodeWorkerRequest::Encode {
                key: same_key,
                picker: picker.clone(),
                frame: frame(),
                area: newer_area,
                allow_upscale: false,
                class: WorkClass::CriticalCurrent,
                generation: 2,
                enqueued_at: Instant::now(),
            },
            &mut queue
        ));

        assert!(enqueue_encode_request(
            EncodeLaneKind::Current,
            EncodeWorkerRequest::Encode {
                key: same_key,
                picker,
                frame: frame(),
                area: older_area,
                allow_upscale: false,
                class: WorkClass::CriticalCurrent,
                generation: 1,
                enqueued_at: Instant::now(),
            },
            &mut queue
        ));

        let task =
            super::pop_next_encode_task(&mut queue).expect("newer current task should remain");
        assert_eq!(task.key, same_key);
        assert_eq!(task.area, newer_area);
        assert!(super::pop_next_encode_task(&mut queue).is_none());
    }

    #[test]
    fn enqueue_with_notifications_keeps_newer_same_key_when_older_request_arrives_late() {
        let mut queue: PrefetchQueue<TerminalFrameKey, EncodeWorkerTask> =
            PrefetchQueue::new(PrefetchQueueConfig::default());
        let picker = Picker::halfblocks();
        let same_key = key(1);
        let newer_area = Rect::new(0, 0, 10, 6);
        let older_area = Rect::new(0, 0, 6, 4);
        let (tx, mut rx) = unbounded_channel();

        assert!(enqueue_with_notifications(
            EncodeLaneKind::Current,
            EncodeWorkerRequest::Encode {
                key: same_key,
                picker: picker.clone(),
                frame: frame(),
                area: newer_area,
                allow_upscale: false,
                class: WorkClass::CriticalCurrent,
                generation: 2,
                enqueued_at: Instant::now(),
            },
            &mut queue,
            &tx
        ));

        assert!(enqueue_with_notifications(
            EncodeLaneKind::Current,
            EncodeWorkerRequest::Encode {
                key: same_key,
                picker,
                frame: frame(),
                area: older_area,
                allow_upscale: false,
                class: WorkClass::CriticalCurrent,
                generation: 1,
                enqueued_at: Instant::now(),
            },
            &mut queue,
            &tx
        ));

        let task =
            super::pop_next_encode_task(&mut queue).expect("newer current task should remain");
        assert_eq!(task.key, same_key);
        assert_eq!(task.area, newer_area);
        assert!(super::pop_next_encode_task(&mut queue).is_none());

        let queue_state_events = std::iter::from_fn(|| rx.try_recv().ok())
            .filter(|event| matches!(event.event, EncodeWorkerEvent::QueueState { .. }))
            .count();
        assert_eq!(queue_state_events, 1);
    }
}
