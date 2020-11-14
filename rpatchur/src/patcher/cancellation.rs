use super::PatcherCommand;
use tokio::sync::mpsc;

pub type InterruptibleFnResult<T> = std::result::Result<T, InterruptibleFnError>;

pub enum InterruptibleFnError {
    Err(String), // An actual error
    Interrupted, // An interruption
}

pub async fn wait_for_cancellation(
    patching_thread_rx: &mut mpsc::Receiver<PatcherCommand>,
) -> InterruptibleFnError {
    if let Some(cmd) = patching_thread_rx.recv().await {
        match cmd {
            PatcherCommand::Cancel => InterruptibleFnError::Interrupted,
            _ => InterruptibleFnError::Err("Unexpected command received".to_string()),
        }
    } else {
        InterruptibleFnError::Err("Channel was closed".to_string())
    }
}

pub fn check_for_cancellation(
    patching_thread_rx: &mut mpsc::Receiver<PatcherCommand>,
) -> Option<InterruptibleFnError> {
    if let Ok(cmd) = patching_thread_rx.try_recv() {
        match cmd {
            PatcherCommand::Cancel => Some(InterruptibleFnError::Interrupted),
            _ => None,
        }
    } else {
        None
    }
}
