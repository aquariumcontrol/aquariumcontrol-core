/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

/// Contains the functionality for triggering regular DHT sensor communication and caching of data
pub mod sensor_manager;

/// Contains the functionality for triggering regular Atlas Scientific sensor communication and caching of data
pub mod atlas_scientific;

/// Contains communication with DHT sensor hardware
pub mod dht;

/// Contains management of GPIO pins
pub mod gpio_handler;

/// Contains communication with Atlas Scientific sensor via I2C interface
pub mod i2c_interface;

/// Contains the functionality for reading from DS18B20 sensor
pub mod ds18b20;

/// Contains the functionality for reading and postprocessing tank level switch position by reading GPIO pin state
pub mod tank_level_switch;
