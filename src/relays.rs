/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

/// Contains the main functionality for the actuator control
pub mod relay_manager;

/// Contains the trait implementation for the communication with real hardware
pub mod actuate_controllino;

/// Contains the trait implementation for the communication with the test server
pub mod actuate_simulator;

/// Contains the functionality to translate an actuator command into the relay actuation command
pub mod device_relay_logic;

/// Contains the trait implementation for the relay actuation via GPIO
pub mod actuate_gpio;

/// Container for message content sent to Controllino
pub mod controllino_message;

/// Enum codifies the errors which the program may encounter in preparation or execution of communication with the relay manager
pub mod relay_error;

/// Container for channels used by relay manager
pub mod relay_manager_channels;
