/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    use crate::mineral::mineral_injection::MineralInjectionTrait;
    use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
    use std::fmt;
    use std::sync::mpsc::{Receiver, Sender};

    pub struct MockMineralInjection {
        pub(crate) pump1_engagement_durations: Vec<u32>,
        pub(crate) pump2_engagement_durations: Vec<u32>,
        pub(crate) pump3_engagement_durations: Vec<u32>,
        pub(crate) pump4_engagement_durations: Vec<u32>,
        pub(crate) pump_protocol: Vec<AquariumDevice>,
    }

    impl MockMineralInjection {
        // Creates a new `MockMineralInjection` instance.
        //
        // This constructor initializes the mock mineral injector for testing purposes.
        // It sets up empty vectors to record the engagement durations for each of
        // the four peristaltic pumps, allowing tests to verify if and how long
        // each pump was "actuated."
        //
        // # Returns
        // A new `MockMineralInjection` struct, ready to record simulated dosing events.
        pub fn new() -> MockMineralInjection {
            MockMineralInjection {
                pump1_engagement_durations: Vec::new(),
                pump2_engagement_durations: Vec::new(),
                pump3_engagement_durations: Vec::new(),
                pump4_engagement_durations: Vec::new(),
                pump_protocol: Vec::new(),
            }
        }
    }

    impl MineralInjectionTrait for MockMineralInjection {
        // Simulates the injection of minerals by recording pump engagement durations.
        //
        // This mock implementation of `inject_mineral` does not perform any physical dosing.
        // Instead, it serves as a testing utility by:
        // 1. Storing the `dosing_duration_millis` into the appropriate pump's
        //    engagement duration vector (e.g., `pump1_engagement_durations`).
        // 2. Printing a console message for debug visibility during test runs.
        // 3. Always returning `false` for the `quit_command_received` status.
        // 4. Returning the `dosing_duration_millis` as the `actual_dosing_duration_millis` (simulating perfect execution).
        //
        // The channel arguments are ignored in this mock implementation.
        //
        // # Arguments
        // * `_tx_balling_to_relay_manager` - (Ignored) Sender channel for sending commands to the relay manager.
        // * `_rx_balling_from_relay_manager` - (Ignored) Receiver channel for acknowledgments from the relay manager.
        // * `_rx_balling_from_signal_handler` - (Ignored) Receiver channel for listening to signals from the signal handler.
        // * `mineral_pump` - The `AquariumDevice` enum variant representing the peristaltic pump to be "actuated."
        // * `dosing_duration_millis` - The target duration (in milliseconds) for which the pump would run.
        //
        // # Returns
        // A tuple `(bool, u32)`:
        // - The `bool` is always `false`, indicating no `Quit` command was received.
        // - The `u32` is the `dosing_duration_millis` passed in, simulating the actual duration.
        //
        // # Panics
        // This function will panic if invoked with an `AquariumDevice` that is not
        // one of the peristaltic pumps (e.g., `AquariumDevice::Skimmer`).
        fn inject_mineral(
            &mut self,
            _tx_balling_to_relay_manager: &Sender<InternalCommand>,
            _rx_balling_from_relay_manager: &Receiver<bool>,
            _rx_balling_from_signal_handler: &Receiver<InternalCommand>,
            mineral_pump: AquariumDevice,
            dosing_duration_millis: u32,
        ) -> (bool, u32) {
            // do nothing except for recording the dosing duration for the specific pump and
            // inserting the pump id in the protocol vector
            match mineral_pump {
                AquariumDevice::PeristalticPump1 => {
                    self.pump1_engagement_durations.push(dosing_duration_millis);
                }
                AquariumDevice::PeristalticPump2 => {
                    self.pump2_engagement_durations.push(dosing_duration_millis);
                }
                AquariumDevice::PeristalticPump3 => {
                    self.pump3_engagement_durations.push(dosing_duration_millis);
                }
                AquariumDevice::PeristalticPump4 => {
                    self.pump4_engagement_durations.push(dosing_duration_millis);
                }
                _ => {
                    panic!("inject_mineral (mock) invoked with invalid device.");
                }
            }
            self.pump_protocol.push(mineral_pump.clone());
            println!(
                "inject_mineral (mock): recording {}ms engagement for {}",
                dosing_duration_millis, mineral_pump,
            );
            (false, dosing_duration_millis)
        }
    }
    
    // Generates a string representation of pump engagement durations.
    //
    // This helper function is used exclusively in test environments to create
    // a human-readable summary of how long a specific pump was "engaged"
    // during simulated dosing events.
    //
    // Arguments:
    // * `id`: The numerical ID of the pump.
    // * `v`: A reference to a vector of `u32` representing the recorded
    //   engagement durations (in milliseconds) for that pump.
    //
    // Returns:
    // A `String` detailing the pump's ID and its recorded engagement durations,
    // or a message indicating that the pump was not engaged.
    fn output_vector(id: u32, v: &Vec<u32>) -> String {
        let mut output: String = "Pump #".into();
        if v.is_empty() {
            output += &id.to_string();
            output += " has not been engaged.";
        } else {
            output += &id.to_string();
            output += " engagement duration(s) (ms): ";
            for duration in v {
                output += &duration.to_string();
                output += " ";
            }
        }
        output
    }

    impl fmt::Display for MockMineralInjection {
        // Formats the `MockMineralInjection` struct to display the recorded dosing durations for each pump.
        //
        // This implementation is primarily for debugging and testing purposes. It provides
        // a human-readable, multi-line string summarizing the "engagement" (simulated dosing)
        // for each of the four peristaltic pumps.
        //
        // # Arguments
        // * `f` - A mutable reference to the formatter, as required by the `fmt::Display` trait.
        //
        // # Returns
        // A `fmt::Result` indicating whether the formatting operation was successful.
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(
                f,
                "{}\n{}\n{}\n{}\n",
                output_vector(1, &self.pump1_engagement_durations),
                output_vector(2, &self.pump2_engagement_durations),
                output_vector(3, &self.pump3_engagement_durations),
                output_vector(4, &self.pump4_engagement_durations),
            )
        }
    }
}