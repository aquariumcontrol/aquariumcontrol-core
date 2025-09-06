/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    use crate::sensors::atlas_scientific_config::AtlasScientificConfig;
    use crate::sensors::i2c_interface::{I2cResponse};
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
    use crate::sensors::i2c_interface_channels::I2cInterfaceChannels;

    pub struct MockI2cInterface {
        /// I2C address of Atlas Scientific pH sensor unit
        address_atlas_scientific_ph: u16,

        /// I2C address of Atlas Scientific conductivity sensor unit
        address_atlas_scientific_conductivity: u16,

        #[allow(unused)]
        /// I2C address of Atlas Scientific temperature sensor unit
        address_atlas_scientific_temperature: u16,

        /// Mock temperature response to be returned when requested
        mock_response_temperature: I2cResponse,

        /// Mock pH response to be returned when requested
        mock_response_ph: I2cResponse,

        /// Mock conductivity response to be returned when requested
        mock_response_conductivity: I2cResponse,
    }

    impl ProcessExternalRequestTrait for MockI2cInterface {}

    impl MockI2cInterface {
        pub fn new(
            atlas_scientific_config: &AtlasScientificConfig,
            mock_response_temperature: I2cResponse,
            mock_response_ph: I2cResponse,
            mock_response_conductivity: I2cResponse,
        ) -> MockI2cInterface {
            MockI2cInterface {
                address_atlas_scientific_ph: atlas_scientific_config.address_atlas_scientific_ph,
                address_atlas_scientific_conductivity: atlas_scientific_config
                    .address_atlas_scientific_conductivity,
                address_atlas_scientific_temperature: atlas_scientific_config
                    .address_atlas_scientific_temperature,
                mock_response_temperature,
                mock_response_ph,
                mock_response_conductivity,
            }
        }

        pub fn execute(
            &mut self,
            i2c_interface_channels: I2cInterfaceChannels
        ) {
            loop {
                let (
                    quit_command_received, // the request to end the application has been received
                    _,
                    _,
                ) = self.process_external_request(&i2c_interface_channels.rx_i2c_interface_from_signal_handler, None);

                if quit_command_received {
                    break;
                }

                match i2c_interface_channels.rx_i2c_interface_from_atlas_scientific.try_recv() {
                    Ok(request) => {
                        println!("{}: Read from channel.", module_path!());
                        let mut response: I2cResponse = self.mock_response_temperature.clone();

                        if request.i2c_address == self.address_atlas_scientific_ph {
                            response = self.mock_response_ph.clone();
                        }
                        if request.i2c_address == self.address_atlas_scientific_conductivity {
                            response = self.mock_response_conductivity.clone();
                        }
                        println!("{}: responding with response={}", module_path!(), response);
                        i2c_interface_channels.tx_i2c_interface_to_atlas_scientific
                            .send(Ok(response))
                            .unwrap();
                    }
                    Err(_) => { /* do nothing */ }
                }

                match i2c_interface_channels.rx_i2c_interface_from_signal_handler.try_recv() {
                    Ok(signal_handler_request) => {
                        if signal_handler_request == InternalCommand::Quit {
                            println!("{}: received Quit command.", module_path!());
                            break;
                        } else {
                            panic!(
                                "{}: received unknown request {} from signal handler.",
                                module_path!(),
                                signal_handler_request
                            );
                        }
                    }
                    _ => { /* no message available, do nothing */ }
                }
            }
        }
    }
}
