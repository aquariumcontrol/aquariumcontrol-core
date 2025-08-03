use crate::utilities::channel_content::InternalCommand;
use std::sync::mpsc::{Receiver, TryRecvError};

#[allow(unused)]
pub fn mock_common_receive_from_signal_handler(
    rx_common_from_signal_handler: &Receiver<InternalCommand>,
    sourced_from: &str,
) -> bool {
    match rx_common_from_signal_handler.try_recv() {
        Ok(c) => match c {
            InternalCommand::Quit => true,
            _ => {
                panic!("{sourced_from}: Received invalid command.");
            }
        },
        Err(e) => match e {
            TryRecvError::Empty => false,
            TryRecvError::Disconnected => {
                panic!(
                    "{sourced_from}: Error in trying to receive command from signal handler: {e:?}"
                );
            }
        },
    }
}
