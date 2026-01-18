/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

pub mod ambient_temperature_time_profile;

pub mod command_parser;

pub mod conductivity_reverse_compensation;

pub mod controllino_emulator;

pub mod evaporation;

pub mod heat_flow;

pub mod light_intensity_time_profile;

pub mod logger;

pub mod model_environment;

#[allow(non_snake_case)]
pub mod pH_time_profile;

pub mod sump;

pub mod tank_geometry;

pub mod tank_level_position;

pub mod tcp_connection;

pub mod test_server_config;

pub mod thermal_model;

pub mod test_server_channel_content;
pub mod tcp_connection_config;