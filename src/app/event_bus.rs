use crossterm::event::EventStream;
use futures_util::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use crate::event::DomainEvent;

pub(crate) struct EventBusRuntime {
    tasks: Vec<JoinHandle<()>>,
}

impl EventBusRuntime {
    pub(crate) fn spawn(
        enable_input: bool,
    ) -> (
        UnboundedSender<DomainEvent>,
        UnboundedReceiver<DomainEvent>,
        Self,
    ) {
        let (tx, rx) = unbounded_channel();
        let tasks = if enable_input {
            vec![spawn_input_task(tx.clone())]
        } else {
            Vec::new()
        };
        (tx, rx, Self { tasks })
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
    fn spawn_without_input_creates_runtime_without_tasks() {
        let (_tx, _rx, mut runtime) = EventBusRuntime::spawn(false);
        runtime.shutdown();
    }
}
