/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use aquarium_control::test_server::conductivity_reverse_compensation::ConductivityReverseCompensation;
use aquarium_control::test_server::test_server_config::read_test_server_config_file;

#[test]
// [itest->swreq~simulator_sensor_signals~1]
// Verifies that no compensation.
fn test_no_compensation() {
    let config = read_test_server_config_file(None);
    let conductivity_compensation = ConductivityReverseCompensation {
        config: config.conductivity_reverse_compensation,
    };
    assert_eq!(
        conductivity_compensation.conductivity_reverse_compensated(25.0),
        50000.0
    );
}

#[test]
// [itest->swreq~simulator_sensor_signals~1]
// Verifies that high temperature.
fn test_high_temperature() {
    let config = read_test_server_config_file(None);
    let conductivity_compensation = ConductivityReverseCompensation {
        config: config.conductivity_reverse_compensation,
    };
    assert_eq!(
        conductivity_compensation.conductivity_reverse_compensated(30.0),
        55250.0
    );
}

#[test]
// [itest->swreq~simulator_sensor_signals~1]
// Verifies that low temperature.
fn test_low_temperature() {
    let config = read_test_server_config_file(None);
    let conductivity_compensation = ConductivityReverseCompensation {
        config: config.conductivity_reverse_compensation,
    };
    assert_eq!(
        conductivity_compensation.conductivity_reverse_compensated(20.0),
        44750.0
    );
}
