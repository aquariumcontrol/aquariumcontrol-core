/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

// Contains functionality to control the heater
pub mod heating;

// Contains one function to check the validity of heating and ventilation configuration
pub mod distinct_interval_check;

// Contains functionality to control the ventilation
pub mod ventilation;

// Container for the channels of heating control used to communicate with other threads
pub mod heating_channels;

// Container for the channels of ventilation control used to communicate with other threads
pub mod ventilation_channels;

// Configuration of Heating
pub mod heating_config;

// Configuration of Ventilation
pub mod ventilation_config;

// Configuration of temperature gradient
pub mod temperature_gradient_config;

// Calculation of temperature gradient
pub mod temperature_gradient;

// Macro for managing cycle time
pub mod manage_cycle_time_thermal;

// Error definition for temperature gradient calculation
pub mod temperature_gradient_error;

// Container for the channels of the temperature gradient calculation used to communicate with other threads
pub mod temperature_gradient_channels;

// Monitor for the temperature gradient
pub mod temperature_gradient_monitor;

// Container for all mutexes used by heating
pub mod heating_mutexes;

// Container for all mutexes used by ventilation
pub mod ventilation_mutexes;

// Trait for update of statistical data in database
pub mod heating_stats_data_transfer_trait;

// Low-level functionality for actuation of heater
pub mod heating_actuation;

// Container for communication from either Heating or Ventilation to DataLogger
pub mod thermal_control_status;
