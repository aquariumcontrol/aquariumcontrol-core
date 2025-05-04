/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use chrono::NaiveDateTime;
use std::fmt;

#[derive(Debug, PartialEq)]
/// Contains the data to be logged wrapped into Option.
/// For each signal, where the data could not be acquired, the None value is used.
/// The None value is represented as NULL in the SQL database.
/// The struct only contains minimal implementation for development purposes.
pub struct RecorderDataFrame {
    /// timestamp when the frame was captured
    pub timestamp: NaiveDateTime,

    /// measured water temperature in °C
    pub water_temperature: Option<f32>,

    /// filtered water temperature in °C
    pub water_temperature_filtered: Option<f32>,

    /// measured pH value of water
    pub ph: Option<f32>,

    /// filtered pH value of water
    pub ph_filtered: Option<f32>,

    /// measured conductivity of water in uS/cm
    pub conductivity: Option<f32>,

    /// filtered conductivity of water in uS/cm
    pub conductivity_filtered: Option<f32>,

    /// conductivity in uS/cm compensated for temperature deviation
    pub conductivity_compensated: Option<f32>,

    /// status bit indicating that fresh water refill is ongoing
    pub refill_in_progress: Option<bool>,

    /// measured tank level switch position (true=high, false=low)
    pub tank_level_switch_position: Option<bool>,

    /// tank level switch validity bit (true=invalid, false=valid)
    pub tank_level_switch_invalid: Option<bool>,

    /// status bit indicating that ventilation is switched on
    pub surface_ventilation_status: Option<bool>,

    /// measured ambient temperature in °C
    pub ambient_temperature: Option<f32>,

    /// measured ambient humidity in %
    pub ambient_humidity: Option<f32>,

    /// status bit indicating that the heater is switched on
    pub heater_status: Option<bool>,

    /// measured water temperature from DS18B20 sensor in °C
    pub water_temperature_ds18b20: Option<f32>,

    /// measured ambient temperature from DS18B20 sensor in °C
    pub ambient_temperature_ds18b20: Option<f32>,
}

/// Provides output of the enum values for debugging purposes.
/// # Arguments
/// * `f` - configuration for formatting
impl fmt::Display for RecorderDataFrame {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let none_string = "None".to_string();
        let water_temperature_string = match self.water_temperature {
            Some(water_temperature) => water_temperature.to_string(),
            None => none_string.clone(),
        };
        let water_temperature_filtered_string = match self.water_temperature_filtered {
            Some(water_temperature_filtered) => water_temperature_filtered.to_string(),
            None => none_string.clone(),
        };
        let ph_string = match self.ph {
            Some(ph) => ph.to_string(),
            None => none_string.clone(),
        };
        let ph_filtered_string = match self.ph_filtered {
            Some(ph_filtered) => ph_filtered.to_string(),
            None => none_string.clone(),
        };
        let conductivity_string = match self.conductivity {
            Some(conductivity) => conductivity.to_string(),
            None => none_string.clone(),
        };
        let conductivity_filtered_string = match self.conductivity_filtered {
            Some(conductivity_filtered) => conductivity_filtered.to_string(),
            None => none_string.clone(),
        };
        let conductivity_compensated_string = match self.conductivity_compensated {
            Some(conductivity_compensated) => conductivity_compensated.to_string(),
            None => none_string.clone(),
        };
        let refill_in_progress_string = match self.refill_in_progress {
            Some(refill_in_progress) => match refill_in_progress {
                true => "yes".to_string(),
                false => "no".to_string(),
            },
            None => none_string.clone(),
        };
        let tank_level_switch_position_string = match self.tank_level_switch_position {
            Some(tank_level_switch_position) => match tank_level_switch_position {
                true => "high".to_string(),
                false => "low".to_string(),
            },
            None => none_string.clone(),
        };
        let tank_level_switch_invalid_string = match self.tank_level_switch_invalid {
            Some(tank_level_switch_invalid) => match tank_level_switch_invalid {
                true => "invalid".to_string(),
                false => "valid".to_string(),
            },
            None => none_string.clone(),
        };
        let surface_ventilation_status_string = match self.surface_ventilation_status {
            Some(surface_ventilation_status) => match surface_ventilation_status {
                true => "on".to_string(),
                false => "off".to_string(),
            },
            None => none_string.clone(),
        };
        let ambient_temperature_string = match self.ambient_temperature {
            Some(ambient_temperature) => ambient_temperature.to_string(),
            None => none_string.clone(),
        };
        let ambient_humidity_string = match self.ambient_humidity {
            Some(ambient_humidity) => ambient_humidity.to_string(),
            None => none_string.clone(),
        };
        let heater_status_string = match self.heater_status {
            Some(heater_status) => match heater_status {
                true => "on".to_string(),
                false => "off".to_string(),
            },
            None => none_string.clone(),
        };
        let water_temperature_ds18b20_string = match self.water_temperature_ds18b20 {
            Some(water_temperature_ds18b20) => water_temperature_ds18b20.to_string(),
            None => none_string.clone(),
        };
        let ambient_temperature_ds18b20_string = match self.ambient_temperature_ds18b20 {
            Some(ambient_temperature_ds18b20) => ambient_temperature_ds18b20.to_string(),
            None => none_string.clone(),
        };
        write!(
            f,
            "\n**************************************\ntimestamp: {}\
            \nwater_temperature: {},\nwater_temperature_filtered: {}, \
            \nph: {},\nph_filtered: {}, \
            \nconductivity: {},\nconductivity_filtered: {},\nconductivity_compensated: {}, \
            \nrefill_in_progress: {}, \
            \ntank_level_switch_position: {},\ntank_level_switch_invalid: {},\
            \nsurface_ventilation_status: {},\
            \nambient_temperature: {},\nambient_humidity: {}, \
            \nheater_status: {} \
            \nwater_temperature_ds18b20: {}, ambient_temperature_ds18b20: {}\n
            **************************************\n",
            self.timestamp,
            water_temperature_string,
            water_temperature_filtered_string,
            ph_string,
            ph_filtered_string,
            conductivity_string,
            conductivity_filtered_string,
            conductivity_compensated_string,
            refill_in_progress_string,
            tank_level_switch_position_string,
            tank_level_switch_invalid_string,
            surface_ventilation_status_string,
            ambient_temperature_string,
            ambient_humidity_string,
            heater_status_string,
            water_temperature_ds18b20_string,
            ambient_temperature_ds18b20_string,
        )
    }
}

#[cfg(test)]
impl RecorderDataFrame {
    pub fn assert(
        &self,
        target_water_temperature: Option<f32>,
        target_water_temperature_filtered: Option<f32>,
        target_water_ph: Option<f32>,
        target_water_ph_filtered: Option<f32>,
        target_water_conductivity: Option<f32>,
        target_water_conductivity_filtered: Option<f32>,
        target_water_conductivity_compensated: Option<f32>,
        target_refill_in_progress: Option<bool>,
        target_tank_level_switch_position: Option<bool>,
        target_tank_level_switch_invalid: Option<bool>,
        target_surface_ventilation_status: Option<bool>,
        target_ambient_temperature: Option<f32>,
        target_ambient_humidity: Option<f32>,
        target_heater_status: Option<bool>,
        target_water_temperature_ds18b20: Option<f32>,
        target_ambient_temperature_ds18b20: Option<f32>,
    ) {
        assert_eq!(self.water_temperature, target_water_temperature);
        assert_eq!(
            self.water_temperature_filtered,
            target_water_temperature_filtered,
        );
        assert_eq!(self.ph, target_water_ph);
        assert_eq!(self.ph_filtered, target_water_ph_filtered);
        assert_eq!(self.conductivity, target_water_conductivity);
        assert_eq!(
            self.conductivity_filtered,
            target_water_conductivity_filtered
        );
        assert_eq!(
            self.conductivity_compensated,
            target_water_conductivity_compensated
        );
        assert_eq!(self.refill_in_progress, target_refill_in_progress);
        assert_eq!(
            self.tank_level_switch_position,
            target_tank_level_switch_position
        );
        assert_eq!(
            self.tank_level_switch_invalid,
            target_tank_level_switch_invalid
        );
        assert_eq!(
            self.surface_ventilation_status,
            target_surface_ventilation_status
        );
        assert_eq!(self.ambient_temperature, target_ambient_temperature);
        assert_eq!(self.ambient_humidity, target_ambient_humidity);
        assert_eq!(self.heater_status, target_heater_status);
        assert_eq!(
            self.water_temperature_ds18b20,
            target_water_temperature_ds18b20
        );
        assert_eq!(
            self.ambient_temperature_ds18b20,
            target_ambient_temperature_ds18b20
        );
    }
}
