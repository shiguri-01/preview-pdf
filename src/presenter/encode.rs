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
use crate::render::prefetch::{PrefetchClass, PrefetchQueue, PrefetchQueueConfig, QueueTaskMeta};

use super::image_ops::{create_protocol_with_picker, downscale_frame_for_area};
use super::l2_cache::TerminalFrameKey;

pub(crate) const ENCODE_RESIZE_FILTER: FilterType = FilterType::Nearest;

pub(crate) enum EncodeWorkerRequest {
    Encode {
        key: TerminalFrameKey,
        picker: Picker,
        frame: RgbaFrame,
        area: Rect,
        class: PrefetchClass,
        generation: u64,
    },
    Shutdown,
}

pub(crate) struct EncodeWorkerTask {
    pub(crate) key: TerminalFrameKey,
    pub(crate) picker: Picker,
    pub(crate) frame: RgbaFrame,
    pub(crate) area: Rect,
}

pub(crate) struct EncodeWorkerResult {
    pub(crate) key: TerminalFrameKey,
    pub(crate) protocol: Option<StatefulProtocol>,
    pub(crate) elapsed: std::time::Duration,
    pub(crate) succeeded: bool,
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
) -> Result<(), EncodeWorkerRequest> {
    let Some(request_tx) = request_tx.as_ref() else {
        return Err(request);
    };
    request_tx.send(request).map_err(|err| err.0)
}

pub(crate) fn spawn_encode_worker(
    runtime: &EncodeWorkerRuntime,
) -> (
    UnboundedSender<EncodeWorkerRequest>,
    UnboundedReceiver<EncodeWorkerResult>,
    JoinHandle<()>,
) {
    let (request_tx, request_rx) = unbounded_channel();
    let (result_tx, result_rx) = unbounded_channel();
    let worker = runtime.spawn_blocking(move || encode_worker_main(request_rx, result_tx));
    (request_tx, result_rx, worker)
}

pub(crate) fn enqueue_encode_request(
    request: EncodeWorkerRequest,
    queue: &mut PrefetchQueue<TerminalFrameKey, EncodeWorkerTask>,
) -> bool {
    match request {
        EncodeWorkerRequest::Encode {
            key,
            picker,
            frame,
            area,
            class,
            generation,
        } => {
            let _ = queue.cancel_stale_prefetch(generation);
            if class == PrefetchClass::CriticalCurrent && queue.contains_key(&key) {
                let _ = queue.retain(|_, meta| meta.key != key);
            }

            let task = EncodeWorkerTask {
                key,
                picker,
                frame,
                area,
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

pub(crate) fn pop_next_encode_task(
    queue: &mut PrefetchQueue<TerminalFrameKey, EncodeWorkerTask>,
) -> Option<EncodeWorkerTask> {
    queue.pop_next()
}

fn encode_worker_main(
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
            if !enqueue_encode_request(request, &mut queue) {
                break;
            }
        }

        loop {
            match request_rx.try_recv() {
                Ok(request) => {
                    if !enqueue_encode_request(request, &mut queue) {
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

        let started = std::time::Instant::now();
        let frame = match downscale_frame_for_area(task.frame, task.area, task.picker.font_size()) {
            Ok(frame) => frame,
            Err(_) => {
                let _ = result_tx.send(EncodeWorkerResult {
                    key: task.key,
                    protocol: None,
                    elapsed: started.elapsed(),
                    succeeded: false,
                });
                continue;
            }
        };
        let mut protocol = match create_protocol_with_picker(&task.picker, frame) {
            Ok(protocol) => protocol,
            Err(_) => {
                let _ = result_tx.send(EncodeWorkerResult {
                    key: task.key,
                    protocol: None,
                    elapsed: started.elapsed(),
                    succeeded: false,
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
            key: task.key,
            protocol: if succeeded { Some(protocol) } else { None },
            elapsed: started.elapsed(),
            succeeded,
        });
    }
}
