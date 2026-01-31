/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#![allow(non_snake_case)]

use crate::test_server::thermal_model::{TankTemperatureModel, TankTemperatureModelConfig};
use chrono::{NaiveTime, Utc};
use spin_sleep::SpinSleeper;
use std::sync::mpsc::Receiver;
use std::time::Duration;
use crate::test_server::ambient_temperature_time_profile::AmbientTemperatureTimeProfileConfig;
use crate::test_server::conductivity_reverse_compensation::{ConductivityReverseCompensation, ConductivityReverseCompensationConfig};
use crate::test_server::evaporation::EvaporationConfig;
use crate::test_server::heat_flow::HeatFlowConfig;
use crate::test_server::light_intensity_time_profile::LightIntensityTimeProfileConfig;
use crate::test_server::logger::SenderLogger;
use crate::test_server::pH_time_profile::{pHTimeProfile, pHTimeProfileConfig};
use crate::test_server::sump::{PumpStatus, Sump, SumpConfig};
use crate::test_server::tank_geometry::{TankGeometry, TankGeometryConfig};
use crate::test_server::tank_level_position::{TankLevelPosition, TankLevelPositionConfig};
use crate::test_server::test_server_channel_content::{BallingFluidInjection, TestServerInternalCommand};
use crate::utilities::channel_content::{AquariumDevice, AquariumSignal};

pub struct ModelEnvironmentConfig {
    pub tank_geometry: TankGeometryConfig,
    pub sump: SumpConfig,
    pub tank_level_switch: TankLevelPositionConfig,
    pub ph_time_profile: pHTimeProfileConfig,
    pub tank_temperature_model: TankTemperatureModelConfig,
    pub heat_flow: HeatFlowConfig,
    pub light_intensity_time_profile: LightIntensityTimeProfileConfig,
    pub ambient_temperature_time_profile: AmbientTemperatureTimeProfileConfig,
    pub evaporation: EvaporationConfig,
    pub conductivity_reverse_compensation: ConductivityReverseCompensationConfig,
    pub initial_time: NaiveTime,
    pub time_step: f32,
}

/// This struct contains the configuration data for the high-level models and the implementation of the high-level model execution.
pub struct ModelEnvironment {
    /// tank geometry model
    pub tank_geometry: TankGeometry,

    /// water temperature model
    pub tank_temperature_model: TankTemperatureModel,

    /// pH time profile
    pub pH_time_profile: pHTimeProfile,

    /// tank level switch model
    pub tank_level_switch: TankLevelPosition,

    /// conductivity reverse compensation model
    pub conductivity_reverse_compensation: ConductivityReverseCompensation,

    /// time step in seconds (in case simulation shall run faster than real time)
    pub time_step: f32,
}

impl ModelEnvironment {
    /// Function returns a variable of type ModelEnvironment after having initialized all models contained in the same.
    /// # Arguments:
    ///
    /// * `config` - configuration of the model environment
    ///
    /// Limitation: this function takes a NaiveTime as an argument in order to calculate an unfiltered pH value which is used to initialize the pH filter.
    /// If parser requests are processed significantly later than the creation of this struct they will have
    /// seemingly unexplainable results.
    /// The pH signal is not used for any dynamic control circuits. So this limitation will not have impact on functionality of the control application.
    /// However, any functionality implemented additionally should bear this limitation in mind.
    pub fn new(config: ModelEnvironmentConfig) -> ModelEnvironment {
        let tank_geometry = TankGeometry::new(config.tank_geometry);

        let sump = Sump::new(config.sump, config.time_step);

        let tank_level_switch = TankLevelPosition::new(config.tank_level_switch, sump);

        let pH_time_profile =
            pHTimeProfile::new(config.ph_time_profile, config.initial_time, config.time_step);

        let tank_temperature_model = TankTemperatureModel::new(
            config.tank_temperature_model,
            config.heat_flow,
            config.light_intensity_time_profile,
            config.ambient_temperature_time_profile,
            config.evaporation,
            config.time_step,
        );

        let conductivity_reverse_compensation =
            ConductivityReverseCompensation::new(config.conductivity_reverse_compensation);

        if config.time_step == 0.0 {
            panic!(
                "Model environment received invalid time step value {}.",
                config.time_step
            );
        }

        ModelEnvironment {
            tank_geometry,
            tank_temperature_model,
            pH_time_profile,
            tank_level_switch,
            conductivity_reverse_compensation,
            time_step: config.time_step,
        }
    }

    /// Function executes the model calculations, waits for fixed time period, outputs some model data, checks termination condition and processes requests received from channel including responding on requests via channel.
    /// Function does not return any value.
    /// # Arguments:
    ///
    /// * `max_loop_count_opt` - optional value indicating maximum number of loops to execute
    /// * `rx_from_parser` - receiver for communication with other thread
    /// * `tx_to_parser` - sender for communication with other thread
    /// * `time_of_day` - time of day for which to execute the simulation
    /// * `time_of_day_initial` - initial time from which simulation shall start
    pub fn execute_models(
        &mut self,
        max_loop_count_opt: Option<u64>,
        rx_from_parser: Receiver<TestServerInternalCommand>,
        tx_to_parser: SenderLogger<f32>,
        time_of_day_initial: NaiveTime,
    ) {
        let sleep_duration = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();

        let mut i: u64 = 0;

        // initialize time of day with value requested (not limited to local time)
        let mut time_of_day: NaiveTime = time_of_day_initial;

        Utc::now().naive_utc().time();
        loop {
            // record local time at start of simulation iteration
            let time_of_day_start_simulation: NaiveTime = Utc::now().naive_utc().time();

            // execute model calculations every 1000 milliseconds == once per second
            if i.is_multiple_of(1000) {
                self.tank_temperature_model.temperature_iteration(
                    &mut self.tank_geometry,
                    time_of_day,
                    self.time_step,
                );

                self.tank_level_switch
                    .sump
                    .calc_refill(&mut self.tank_geometry, self.time_step);
            }

            spin_sleeper.sleep(sleep_duration);

            i += 1; // counter for reducing frequency of calculation and frequency of logging

            // execute logging every 10000 milliseconds == once every 10 seconds
            if i.is_multiple_of(1000) {
                println!(
                    "Water temperature = {} deg C",
                    self.tank_temperature_model.temperature_water
                );
                println!("Current sump level={}m, Switch configured at HIGH={}m LOW={}m, main pump delta={}",
                        self.tank_level_switch.sump.get_water_level_unfiltered(&self.tank_geometry),
                        self.tank_level_switch.config.water_level_high,
                        self.tank_level_switch.config.water_level_low,
                        self.tank_level_switch.sump.calc_main_pump_delta_level()
                        );
            }

            // check termination condition for simulation
            match max_loop_count_opt {
                None => {} // no limit provided.
                Some(max_loop_count) => {
                    // limit provided. Check if loop shall be aborted.
                    if i > max_loop_count * 1000 {
                        break;
                    }
                }
            };

            // process channel every millisecond, because there can be lots of requests
            if self.process_channel(&rx_from_parser, &tx_to_parser.clone(), time_of_day) {
                return;
            }

            // execute with 1 Hz
            if i.is_multiple_of(1000) {
                // record local time at end of simulation iteration
                let time_of_day_end_simulation: NaiveTime = Utc::now().naive_utc().time();

                // calculate time delta since start of simulation and add this to the time of day used for next iteration of simulation
                time_of_day += time_of_day_end_simulation - time_of_day_start_simulation;
            }
        }
    }

    /// Process information in channel until there is no more data available. Return true value if termination is requested, otherwise false.
    /// # Arguments:
    ///
    /// * `rx_from_parser` - reference to receiver for communication with other thread
    /// * `tx_to_parser` - reference to sender for communication with other thread
    fn process_channel(
        &mut self,
        rx_from_parser: &Receiver<TestServerInternalCommand>,
        tx_to_parser: &SenderLogger<f32>,
        time_of_day: NaiveTime,
    ) -> bool {
        // println!("process_channel invoked with time_of_day = {}", time_of_day);
        // process channel content repeatedly
        loop {
            let Ok(parser_request) = rx_from_parser.try_recv() else {
                // exit from loop if there is no request
                return false;
            };
            if let TestServerInternalCommand::Quit = parser_request {
                // or exit from loop if termination request has been received
                return true;
            }
            // parse channel content
            match self.process_parser_request(parser_request, time_of_day) {
                Some(value) => {
                    // send response value
                    tx_to_parser.send_and_log(value).unwrap();
                }
                None => {
                    // do nothing
                }
            }
        }
    }

    /// Function processes the requests received from the command parser via channel and returns an optional float value.
    ///    
    /// # Arguments:
    ///
    /// * `internal_command` - enum describing the request and potentially containing a wrapped value
    fn process_parser_request(
        &mut self,
        internal_command: TestServerInternalCommand,
        time_of_day: NaiveTime,
    ) -> Option<f32> {
        match internal_command {
            TestServerInternalCommand::SwitchOn(aquarium_device) => {
                match aquarium_device {
                    AquariumDevice::Skimmer => {
                        //println!("Received request to switch on skimmer.");
                    }
                    AquariumDevice::MainPump1 => {
                        //println!("Received request to switch on main pump 1.");
                        self.tank_level_switch.sump.main_pump_1.status = PumpStatus::On;
                    }
                    AquariumDevice::MainPump2 => {
                        //println!("Received request to switch on main pump 2.");
                        self.tank_level_switch.sump.main_pump_2.status = PumpStatus::On;
                    }
                    AquariumDevice::AuxPump1 => {
                        //println!("Received request to switch on aux pump 1.");
                    }
                    AquariumDevice::AuxPump2 => {
                        //println!("Received request to switch on aux pump 2.");
                    }
                    AquariumDevice::Heater => {
                        //println!("Received request to switch on heater.");
                        self.tank_temperature_model.heater_is_on = true;
                    }
                    AquariumDevice::Ventilation => {
                        //println!("Received request to switch on ventilation.");
                        self.tank_temperature_model.surface_ventilation_is_on = true;
                    }
                    AquariumDevice::RefillPump => {
                        println!("Received request to switch on refill pump. Current sump level={}m, Switch configured at HIGH={}m LOW={}m, main pump delta={}",
                        self.tank_level_switch.sump.get_water_level(&self.tank_geometry),
                        self.tank_level_switch.config.water_level_high,
                        self.tank_level_switch.config.water_level_low,
                        self.tank_level_switch.sump.calc_main_pump_delta_level()
                        );

                        self.tank_level_switch.sump.refill_pump.status = PumpStatus::On;
                    }
                    AquariumDevice::Feeder => {
                        //println!("Received request to switch on feeder");
                    }
                    AquariumDevice::PeristalticPump1 => {
                        // Unforeseen use case (permanently switching on peristaltic pump) @todo: add proper reaction
                    }
                    AquariumDevice::PeristalticPump2 => {
                        // Unforeseen use case (permanently switching on peristaltic pump) @todo: add proper reaction
                    }
                    AquariumDevice::PeristalticPump3 => {
                        // Unforeseen use case (permanently switching on peristaltic pump) @todo: add proper reaction
                    }
                    AquariumDevice::PeristalticPump4 => {
                        // Unforeseen use case (permanently switching on peristaltic pump) @todo: add proper reaction
                    }
                    _ => {
                        // Do nothing
                    }
                };
                None
            }
            TestServerInternalCommand::SwitchOff(aquarium_device) => {
                match aquarium_device {
                    AquariumDevice::Skimmer => {
                        //println!("Received request to switch off skimmer.");
                    }
                    AquariumDevice::MainPump1 => {
                        //println!("Received request to switch off main pump 1.");
                        self.tank_level_switch.sump.main_pump_1.status = PumpStatus::Off;
                    }
                    AquariumDevice::MainPump2 => {
                        //println!("Received request to switch off main pump 2.");
                        self.tank_level_switch.sump.main_pump_2.status = PumpStatus::Off;
                    }
                    AquariumDevice::AuxPump1 => {
                        //println!("Received request to switch off aux pump 1.");
                    }
                    AquariumDevice::AuxPump2 => {
                        //println!("Received request to switch off aux pump 2.");
                    }
                    AquariumDevice::Heater => {
                        //println!("Received request to switch off heater.");
                        self.tank_temperature_model.heater_is_on = false;
                    }
                    AquariumDevice::Ventilation => {
                        //println!("Received request to switch off ventilation.");
                        self.tank_temperature_model.surface_ventilation_is_on = false;
                    }
                    AquariumDevice::RefillPump => {
                        println!("Received request to switch off refill pump. Current sump level={}m, Switch configured at HIGH={}m LOW={}m main pump delta={}",
                        self.tank_level_switch.sump.get_water_level(&self.tank_geometry),
                        self.tank_level_switch.config.water_level_high,
                        self.tank_level_switch.config.water_level_low,
                        self.tank_level_switch.sump.calc_main_pump_delta_level()
                        );
                        self.tank_level_switch.sump.refill_pump.status = PumpStatus::Off;
                    }
                    AquariumDevice::Feeder => {
                        //println!("Received request to switch off feeder");
                    }
                    AquariumDevice::PeristalticPump1 => {
                        // Unforeseen use case (permanently switching on peristaltic pump) @todo: add proper reaction
                    }
                    AquariumDevice::PeristalticPump2 => {
                        // Unforeseen use case (permanently switching on peristaltic pump) @todo: add proper reaction
                    }
                    AquariumDevice::PeristalticPump3 => {
                        // Unforeseen use case (permanently switching on peristaltic pump) @todo: add proper reaction
                    }
                    AquariumDevice::PeristalticPump4 => {
                        // Unforeseen use case (permanently switching on peristaltic pump) @todo: add proper reaction
                    }
                    _ => {}
                };
                None
            }
            TestServerInternalCommand::Inject(balling_fluid) => {
                match balling_fluid {
                    BallingFluidInjection::Calcium(_injection_duration) => {
                        // @todo: actuate inverse model for peristaltic pump
                    }
                    BallingFluidInjection::Magnesium(_injection_duration) => {
                        // @todo: actuate inverse model for peristaltic pump
                    }
                    BallingFluidInjection::KH(_injection_duration) => {
                        // @todo: actuate inverse model for peristaltic pump
                    }
                    BallingFluidInjection::NotConnected => {
                        // No action required here.
                    }
                };
                None
            }
            TestServerInternalCommand::RequestSignal(aquarium_signal) => {
                match aquarium_signal {
                    AquariumSignal::WaterTemperature => {
                        return Some(self.tank_temperature_model.temperature_water);
                    }
                    AquariumSignal::pH => {
                        return Some(self.pH_time_profile.pH(time_of_day));
                    }
                    AquariumSignal::Conductivity => {
                        return Some(
                            self.conductivity_reverse_compensation
                                .conductivity_reverse_compensated(
                                    self.tank_temperature_model.temperature_water,
                                ),
                        );
                    }
                    AquariumSignal::AmbientTemperature => {
                        //println!("Received request for ambient temperature.");
                        return Some(
                            self.tank_temperature_model
                                .ambient_temperature_time_profile
                                .ambient_temperature(time_of_day),
                        );
                    }
                    AquariumSignal::AmbientHumidity => {
                        let default_humidity: f32 = 60.0;
                        // todo: implement configuration/model for humidity
                        //println!("Received request for ambient humidity.");
                        return Some(default_humidity);
                    }
                    AquariumSignal::TankLevelSwitchPosition => {
                        //println!("Received request for tank level switch position.");
                        return if self
                            .tank_level_switch
                            .update_and_get_switch_position(&self.tank_geometry)
                        {
                            println!("ModelEnvironment: Communicating tank level switch HIGH");
                            Some(1.0)
                        } else {
                            println!("ModelEnvironment: Communicating tank level switch LOW");
                            Some(0.0)
                        };
                    }
                    AquariumSignal::TankLevelSwitchInvalid => {
                        //println!("Received request for tank level switch invalid.");
                        return if self.tank_level_switch.tank_level_switch_invalid() {
                            Some(1.0)
                        } else {
                            Some(0.0)
                        };
                    }
                    _ => {}
                };
                None
            }
            TestServerInternalCommand::Quit => unreachable!(),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::test_server::command_parser::tests::SenderLoggerTuple;
    use crate::test_server::model_environment::{ModelEnvironment, ModelEnvironmentConfig};
    use crate::test_server::test_server_config::read_test_server_config_file;
    use assert_float_eq::*;
    use chrono::NaiveTime;
    use std::sync::mpsc;

    #[test]
    fn test_model_environment_execution() {
        let config = read_test_server_config_file(None);
        let time_of_day = NaiveTime::from_hms_opt(9, 0, 0).unwrap();

        let model_config = ModelEnvironmentConfig {
            tank_geometry: config.tank_geometry,
            sump: config.sump,
            tank_level_switch: config.tank_level_switch,
            ph_time_profile: config.pH_time_profile,
            tank_temperature_model: config.tank_temperature_model,
            heat_flow: config.heat_flow,
            light_intensity_time_profile: config.light_intensity_time_profile,
            ambient_temperature_time_profile: config.ambient_temperature_time_profile,
            evaporation: config.evaporation,
            conductivity_reverse_compensation: config.conductivity_reverse_compensation,
            initial_time: time_of_day,
            time_step: 1.0,
        };
        let mut model_environment = ModelEnvironment::new(model_config);

        let (_command_tx, command_rx) = mpsc::channel();
        let (model_tx, _model_rx) = mpsc::channel();

        let sender_loggers = SenderLoggerTuple::new(&"/tmp/aquarium_test_server__tests__model_environment__test_model_environment_execution.log", _command_tx, model_tx).unwrap();
        let (_command_tx, model_tx) = sender_loggers.sender_loggers();

        model_environment.execute_models(Some(3), command_rx, model_tx, time_of_day); // execute 3 iterations

        assert_float_absolute_eq!(
            model_environment.tank_temperature_model.temperature_water,
            25.000,
            0.001
        );
    }
}
