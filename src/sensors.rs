/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

// Contains the functionality for triggering regular DHT sensor communication and caching of data
pub mod sensor_manager;

// Contains the functionality for triggering regular Atlas Scientific sensor communication and caching of data
pub mod atlas_scientific;

// Contains communication with DHT sensor hardware
pub mod dht;

// Contains management of GPIO pins
pub mod gpio_handler;

// Contains communication with Atlas Scientific sensor via I2C interface
pub mod i2c_interface;

// Contains the functionality for reading from DS18B20 sensor
pub mod ds18b20;

// Contains the functionality for reading and postprocessing tank level switch position by reading GPIO pin state
pub mod tank_level_switch;

// Contains error definition for Atlas Scientific sensor communication
pub mod atlas_scientific_error;

// Contains error definition for DHT sensor communication
pub mod dht_error;

// Contains error definition for DS18B20 sensor communication
pub mod ds18b20_error;

// Contains error definition occurring in communication via I2C
pub mod i2c_error;

// Container for channels used by atlas_scientific module
pub mod atlas_scientific_channels;

// Container for channels used by i2c_interface module
pub mod i2c_interface_channels;

// Container for channels used by sensor manager module
pub mod sensor_manager_channels;

// Container for channels used by tank level switch module
pub mod tank_level_switch_channels;

// Container for channels used by Ds18b20 module
pub mod ds18b20_channels;

// Container for channels used by Dht module
pub mod dht_channels;

// Configuration of AtlasScientific
pub mod atlas_scientific_config;

// Configuration of Dht
pub mod dht_config;

// Configuration of Ds18b20
pub mod ds18b20_config;

// Configuration of I2cInterface
pub mod i2c_interface_config;

// Configuration of SensorManager
pub mod sensor_manager_config;

// Configuration of TankLevelSwitch
pub mod tank_level_switch_config;

// Configuration of GpioHandler
pub mod gpio_handler_config;

// Trait for communication with Atlas Scientific sensor
pub mod atlas_scientific_driver;

// Trait implementation for EZO type sensor
pub(crate) mod atlas_scientific_ezo;

// Trait implementation for OEM type sensor
pub(crate) mod atlas_scientific_oem;
