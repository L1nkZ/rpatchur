use super::PatcherCommand;

pub type InterruptibleFnResult<T> = std::result::Result<T, InterruptibleFnError>;

pub enum InterruptibleFnError {
    Err(String), // An actual error
    Interrupted, // An interruption
}

pub async fn wait_for_cancellation(
    patching_thread_rx: &mut flume::Receiver<PatcherCommand>,
) -> InterruptibleFnError {
    if let Ok(cmd) = patching_thread_rx.recv_async().await {
        match cmd {
            PatcherCommand::CancelUpdate | PatcherCommand::Quit => {
                InterruptibleFnError::Interrupted
            }
            _ => InterruptibleFnError::Err("Unexpected command received".to_string()),
        }
    } else {
        InterruptibleFnError::Err("Channel was closed".to_string())
    }
}

pub fn process_incoming_commands(
    patching_thread_rx: &mut flume::Receiver<PatcherCommand>,
) -> InterruptibleFnResult<()> {
    match patching_thread_rx.try_recv() {
        Ok(cmd) => match cmd {
            PatcherCommand::CancelUpdate | PatcherCommand::Quit => {
                Err(InterruptibleFnError::Interrupted)
            }
            _ => Ok(()),
        },
        Err(e) => match e {
            flume::TryRecvError::Disconnected => {
                Err(InterruptibleFnError::Err("Channel was closed".to_string()))
            }
            flume::TryRecvError::Empty => Ok(()),
        },
    }
}
