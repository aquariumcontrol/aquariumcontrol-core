/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#[cfg(test)]
#[cfg(target_os = "linux")]
pub mod tests {
    use crate::launch::channels::AquaReceiver;
    use crate::utilities::channel_content::InternalCommand;
    /// Performs the `mock_message_command_receiver` operation.
    ///
    /// # Arguments
    /// * `rx_message_command` - A parameter representing `rx_message_command`.
    pub fn mock_message_command_receiver(
        mut rx_message_command: AquaReceiver<InternalCommand>,
    ) -> Vec<InternalCommand> {
        let mut commands_received: Vec<InternalCommand> = Vec::new();

        loop {
            match rx_message_command.recv() {
                Ok(command) => match command {
                    InternalCommand::Quit => {
                        // println!("{}: received quit command", module_path!());
                        break;
                    }
                    _ => {
                        // println!("{}: received command {}", module_path!(), command);
                        commands_received.push(command);
                    }
                },
                Err(e) => {
                    panic!(
                        "{}: error occurred when receiving message from message thread via channel ({e:?}).",
                        module_path!()
                    );
                }
            }
        }

        commands_received
    }
}
