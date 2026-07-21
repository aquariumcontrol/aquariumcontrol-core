/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use aquarium_control::test_server::controllino_emulator::ControllinoEmulator;
use aquarium_control::test_server::test_server_config::read_test_server_config_file;
#[test]
// Verifies that set unset relay.
fn test_set_unset_relay() {
    let config = read_test_server_config_file(None);

    let mut controllino = ControllinoEmulator {
        relays: vec![
            false, false, false, false, false, false, false, false, false, false, false, false,
            false, false, false, false,
        ],
        config: &config.controllino,
    };

    // checking initial status
    assert_eq!(controllino.get_status(), 0);

    // switching on 5 bits which could represent the flow pumps and the skimmer
    controllino.set_relay(0.try_into().unwrap());
    controllino.set_relay(1.try_into().unwrap());
    controllino.set_relay(2.try_into().unwrap());
    controllino.set_relay(3.try_into().unwrap());
    controllino.set_relay(4.try_into().unwrap());

    assert_eq!(controllino.get_status(), 31);

    // switching off 5 bits which could represent the flow pumps and the skimmer
    controllino.unset_relay(0.try_into().unwrap());
    controllino.unset_relay(1.try_into().unwrap());
    controllino.unset_relay(2.try_into().unwrap());
    controllino.unset_relay(3.try_into().unwrap());
    controllino.unset_relay(4.try_into().unwrap());

    assert_eq!(controllino.get_status(), 0);
}

//TODO: uncomment when heartbeat is implemented
// #[test]
// fn test_heartbeat() {
//     // executes heartbeat and checks if final state is correct

//     let config = read_config_file("/config-test/aquarium_control_test_server.toml".to_string());

//     let mut controllino = Controllino {
//         relays: vec![
//             false, false, false, false, false, false, false, false, false, false, false, false,
//             false, false, false, false,
//         ],
//         config: &config.controllino,
//     };

//     // with unset relay as initial state
//     controllino.heartbeat();
//     assert_eq!(controllino.get_status(), 0);

//     // with set relay as initial state
//     controllino.set_relay(controllino.config.heartbeat_id);
//     controllino.heartbeat();
//     assert_eq!(controllino.get_status(), 0);
// }
