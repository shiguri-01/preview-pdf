use crossterm::event::EventStream;
use futures_util::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

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
        self.tasks.push(spawn_input_task(tx));
    }

    pub(crate) fn shutdown(&mut self) {
        for task in self.tasks.drain(..) {
            task.abort();
        }
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

#[cfg(test)]
mod tests {
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
}
