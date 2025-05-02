/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    use std::fmt::{Display, Formatter};
    use std::time::SystemTime;

    use crate::watchmen::petting::PettingTrait;

    pub struct MockPetting {
        pub petting_recorder: Vec<SystemTime>,
        pub signal_recorder: Vec<String>,
    }

    impl Display for MockPetting {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            writeln!(f, "Watchdog Petting Recorder Timestamps:")?;
            // Check if any timestamps have been recorded
            if self.petting_recorder.is_empty() {
                // If no timestamps, write a message indicating that
                writeln!(f, "(No heartbeats recorded yet)")?;
            } else {
                // Get the first timestamp to calculate relative times
                let start_time = self.petting_recorder[0];

                // Iterate through the recorded timestamps with their index
                for (index, timestamp) in self.petting_recorder.iter().enumerate() {
                    // Calculate the duration since the very first recorded timestamp
                    let duration_since_start = timestamp.duration_since(start_time);
                    // Write the index and the elapsed duration since the start, line by line
                    // Using {:?} for Duration gives a human-readable output (e.g., 1.234s, 50ms)
                    writeln!(
                        f,
                        "  [{:02}]: +{:?} {}",
                        index, duration_since_start, self.signal_recorder[index]
                    )?;
                }
            }
            Ok(())
        }
    }

    impl MockPetting {
        pub fn new() -> MockPetting {
            MockPetting {
                petting_recorder: Vec::new(),
                signal_recorder: Vec::new(),
            }
        }
    }

    impl PettingTrait for MockPetting {
        fn pet(&mut self, _: &str, s: &str) {
            let time = SystemTime::now();
            self.petting_recorder.push(time);
            self.signal_recorder.push(s.to_string());
            // println!("Petting: {:?}", time);
        }
    }
}
