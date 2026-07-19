/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

pub mod tests {
    use crate::launch::channels::{AquaChannelError, AquaReceiver};
    use crate::mocks::mock_common::mock_common_receive_from_signal_handler;
    use crate::sensors::atlas_scientific_monitor_view::AtlasScientificMonitorView;
    use crate::utilities::channel_content::InternalCommand;
    use crate::water::refill_monitor_view::RefillMonitorView;
    use spin_sleep::SpinSleeper;
    use std::fmt;
    use std::time::{Duration, Instant};

    const MOCK_MONITORS_MAX_VEC_LEN: usize = 4096;

    // Simplified version of Monitors for testing refill
    #[derive(Default)]
    pub struct MockMonitors {
        // storage of refill monitor views
        pub refill_monitor_views: Vec<(Instant, RefillMonitorView)>,
        // storage of refill monitor views
        pub atlas_scientific_monitor_views: Vec<(Instant, AtlasScientificMonitorView)>,
    }

    // Implement the Display trait for MockMonitors.
    impl fmt::Display for MockMonitors {
        /// Formats the value using the given formatter.
        ///
        /// # Arguments
        /// * `f` - The formatter to write the formatted string to.
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            if self.refill_monitor_views.is_empty() {
                writeln!(f, "MockMonitors contains no refill monitor views.")?;
            } else {
                writeln!(
                    f,
                    "MockMonitors contains {} refill monitor views:",
                    self.refill_monitor_views.len()
                )?;
                for (i, (instant, view)) in self.refill_monitor_views.iter().enumerate() {
                    // Instant does not implement Display, so we use the Debug format {:?}
                    writeln!(f, "- View {}: (at Instant {:?}) {}", i + 1, instant, view)?;
                }
            }
            if self.atlas_scientific_monitor_views.is_empty() {
                writeln!(
                    f,
                    "MockMonitors contains no Atlas Scientific monitor views."
                )?;
            } else {
                writeln!(
                    f,
                    "MockMonitors contains {} AtlasScientific monitor views:",
                    self.atlas_scientific_monitor_views.len()
                )?;
                for (i, (instant, view)) in self.refill_monitor_views.iter().enumerate() {
                    // Instant does not implement Display, so we use the Debug format {:?}
                    writeln!(f, "- View {}: (at Instant {:?}) {}", i + 1, instant, view)?;
                }
            }
            Ok(())
        }
    }

    impl MockMonitors {
        /// Creates a new instance.
        pub fn new() -> MockMonitors {
            MockMonitors {
                refill_monitor_views: vec![],
                atlas_scientific_monitor_views: vec![],
            }
        }

        /// Performs the `append_refill_monitor_view` operation.
        ///
        /// # Arguments
        /// * `refill_monitor_view` - A parameter representing `refill_monitor_view`.
        pub fn append_refill_monitor_view(&mut self, refill_monitor_view: RefillMonitorView) {
            // Check if the vector has reached or exceeded its maximum allowed length.
            if self.refill_monitor_views.len() >= MOCK_MONITORS_MAX_VEC_LEN {
                // If it has, remove the oldest element (at index 0).
                // This shifts all further elements to the left.
                self.refill_monitor_views.remove(0);
            }
            // Append the new RefillMonitorView struct to the end of the vector.
            self.refill_monitor_views
                .push((Instant::now(), refill_monitor_view));
        }

        /// Performs the `append_atlas_scientific_monitor_view` operation.
        ///
        /// # Arguments
        /// * `atlas_scientific_monitor_view` - A parameter representing `atlas_scientific_monitor_view`.
        pub fn append_atlas_scientific_monitor_view(
            &mut self,
            atlas_scientific_monitor_view: AtlasScientificMonitorView,
        ) {
            // Check if the vector has reached or exceeded its maximum allowed length.
            if self.atlas_scientific_monitor_views.len() >= MOCK_MONITORS_MAX_VEC_LEN {
                // If it has, remove the oldest element (at index 0).
                // This shifts all further elements to the left.
                self.atlas_scientific_monitor_views.remove(0);
            }
            // Append the new RefillMonitorView struct to the end of the vector.
            self.atlas_scientific_monitor_views
                .push((Instant::now(), atlas_scientific_monitor_view));
        }

        /// Executes the main task loop.
        ///
        /// # Arguments
        /// * `rx_from_refill_opt` - A parameter representing `rx_from_refill_opt`.
        /// * `rx_from_atlas_scientific_opt` - A parameter representing `rx_from_atlas_scientific_opt`.
        /// * `rx_from_signal_handler` - Receiver channel for signals from the signal handler.
        pub fn execute(
            &mut self,
            mut rx_from_refill_opt: Option<AquaReceiver<RefillMonitorView>>,
            mut rx_from_atlas_scientific_opt: Option<AquaReceiver<AtlasScientificMonitorView>>,
            mut rx_from_signal_handler: AquaReceiver<InternalCommand>,
        ) {
            let sleep_duration_hundred_millis = Duration::from_millis(100);
            let spin_sleeper = SpinSleeper::default();

            loop {
                if mock_common_receive_from_signal_handler(
                    &mut rx_from_signal_handler,
                    module_path!(),
                ) {
                    break;
                }

                match rx_from_refill_opt {
                    Some(ref mut rx_from_refill) => match rx_from_refill.try_recv() {
                        Ok(refill_monitor_view) => {
                            self.append_refill_monitor_view(refill_monitor_view);
                        }
                        Err(e) => {
                            match e {
                                AquaChannelError::Empty => { /* do nothing */ }
                                AquaChannelError::Disconnected => {
                                    panic!("{}: channel disconnected", module_path!());
                                }
                            }
                        }
                    },
                    None => {
                        // Do nothing
                    }
                }

                match rx_from_atlas_scientific_opt {
                    Some(ref mut rx_from_atlas_scientific) => {
                        match rx_from_atlas_scientific.try_recv() {
                            Ok(atlas_scientific_monitor_view) => {
                                self.append_atlas_scientific_monitor_view(
                                    atlas_scientific_monitor_view,
                                );
                            }
                            Err(e) => {
                                match e {
                                    AquaChannelError::Empty => { /* do nothing */ }
                                    AquaChannelError::Disconnected => {
                                        panic!("{}: channel disconnected", module_path!());
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        // Do nothing
                    }
                }

                spin_sleeper.sleep(sleep_duration_hundred_millis);
            }
        }
    }
}
