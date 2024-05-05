use serde_derive::Deserialize;
use std::io;
use std::{
    io::{prelude::*, BufReader},
    net::TcpStream,
    time::Duration,
};
use spin_sleep::SpinSleeper;

use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

/// This module contains constants representing commands received from control application.
#[allow(dead_code)]
mod command {
    pub const SETSINGLE: &str = "SETSINGLE";
    pub const UNSETSINGLE: &str = "UNSETSINGLE";
    pub const GETSTATUS: &str = "GETSTATUS";
    pub const PULSE: &str = "PULSE";
    pub const HEARTBEAT: &str = "HEARTBEAT";
    pub const READ: &str = "READ";
    pub const INITIALIZED: &str = "INITIALIZED";
}

/// This module contains constants representing the category received from control application.
/// The category describes the device to which the command shall be applied.
#[allow(dead_code)]
mod category {
    pub const CONTROLLINO: &str = "CONTROLLINO";
    pub const DHT22: &str = "DHT22";
    pub const ATLASSCIENTIFIC: &str = "ATLSSCNTFC";
    pub const TANKLEVELSWITCH: &str = "TNKLVLSWTCH";
    pub const REFILLCONTROL: &str = "REFILLCTRL";
}

/// This module contains constants representing the signal request received from control application.
#[allow(dead_code)]
mod signal {
    #[allow(dead_code)]
    pub const AMBIENTTEMPERATURE: &str = "AmbTemp";
    pub const AMBIENTHUMIDITY: &str = "AmbHum";
    pub const TANKLEVELSWITCHPOSITION: &str = "TnkLvlSwtch";
    pub const TANKLEVELSWITCHINVALID: &str = "TnkLvlSwtchInvld";
    pub const ATLASSCIENTIFICTEMPERATURE: &str = "temperature";
    pub const ATLASSCIENTIFICPH: &str = "pH";
    pub const ATLASSCIENTIFICCONDUCTIVITY: &str = "conductivity";
}

/// This struct holds the configuration data for the TCP connection.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct TcpConnectionConfig {
    pub ip_address: String,
    pub port: u16,
}

#[derive(Debug)]
pub enum TcpCommunicationError {
    ConnectionFailed(io::Error),
    //ReadMessageFailed(io::Error),
}

pub struct TcpCommunication {
    write_stream: TcpStream,
    buf_reader: BufReader<TcpStream>,
}

impl TcpCommunication {
    /// split a string into multiple words using space as delimiter and return the last word
    fn get_last_word(sequence: &String) -> Option<&str> {
        let words = sequence.split(" ").collect::<Vec<&str>>();
        words.last().copied()
    }

    /// instantiate a struct of type TcpCommunication
    pub fn new(
        tcp_connection: &TcpConnectionConfig,
    ) -> Result<TcpCommunication, TcpCommunicationError> {
        let stream = match TcpStream::connect(
            tcp_connection.ip_address.clone() + ":" + &tcp_connection.port.to_string(),
        ) {
            Ok(c) => c,
            Err(e) => {
                println!(
                    "Error when trying to connect to {} via port {}: {e:?}",
                    tcp_connection.ip_address, tcp_connection.port
                );
                return Err(TcpCommunicationError::ConnectionFailed(e));
            }
        };
        Ok(TcpCommunication {
            write_stream: stream.try_clone().unwrap(),
            buf_reader: BufReader::new(stream),
        })
    }

    /// function removes trailing newline or carriage return.
    /// Code taken over from <https://stackoverflow.com/questions/37888042/remove-single-trailing-newline-from-string-without-cloning>
    fn trim_newline(s: &mut String) {
        if s.ends_with('\n') {
            s.pop();
            if s.ends_with('\r') {
                s.pop();
            }
        }
    }

    /// communicate with simulator to change relay state
    fn change_relay(&mut self, relay_id: u16, state: bool) {
        let request: String;
        let relay_id_string = relay_id.to_string();
        if state {
            println!("tcp_communication: switching on relay #{}", relay_id);
            request = category::CONTROLLINO.to_string()
                + " "
                + command::SETSINGLE
                + " "
                + &relay_id_string
                + "\n";
        } else {
            println!("tcp_communication: switching off relay #{}", relay_id);
            request = category::CONTROLLINO.to_string()
                + " "
                + command::UNSETSINGLE
                + " "
                + &relay_id_string
                + "\n";
        }
        //println!("Writing...(request={})", request);
        match self.write_stream.write(request.as_bytes()) {
            Ok(_) => {}
            Err(e) => {
                panic!("{e:?}");
            }
        };
        let _ = self.write_stream.flush();

        //println!("Reading...");
        let mut response = String::new();
        response.clear();
        match self.buf_reader.read_line(&mut response) {
            Ok(_) => {}
            Err(e) => {
                panic!("{e:?}");
            }
        };
    }

    /// communicate with simulator to get signal value
    fn get_signal(&mut self, aquarium_signal: AquariumSignal) -> f32 {
        let category = match aquarium_signal {
            AquariumSignal::WaterTemperature => category::ATLASSCIENTIFIC,
            AquariumSignal::pH => category::ATLASSCIENTIFIC,
            AquariumSignal::Conductivity => category::ATLASSCIENTIFIC,
            AquariumSignal::AmbientTemperature => category::DHT22,
            AquariumSignal::AmbientHumidity => category::DHT22,
            AquariumSignal::TankLevelSwitchPosition => category::TANKLEVELSWITCH,
            AquariumSignal::TankLevelSwitchInvalid => category::TANKLEVELSWITCH,
        };
        let tcp_signal_identifier = match aquarium_signal {
            AquariumSignal::WaterTemperature => signal::ATLASSCIENTIFICTEMPERATURE,
            AquariumSignal::pH => signal::ATLASSCIENTIFICPH,
            AquariumSignal::Conductivity => signal::ATLASSCIENTIFICCONDUCTIVITY,
            AquariumSignal::AmbientTemperature => signal::AMBIENTTEMPERATURE,
            AquariumSignal::AmbientHumidity => signal::AMBIENTHUMIDITY,
            AquariumSignal::TankLevelSwitchPosition => signal::TANKLEVELSWITCHPOSITION,
            AquariumSignal::TankLevelSwitchInvalid => signal::TANKLEVELSWITCHINVALID,
        };
        let request =
            category.to_string() + " " + command::READ + " " + tcp_signal_identifier + "\n";

        //println!("Writing...(request={})", request);
        match self.write_stream.write(request.as_bytes()) {
            Ok(_) => {}
            Err(e) => {
                panic!("{e:?}");
            }
        };
        let _ = self.write_stream.flush();

        //println!("Reading...");
        let mut response = String::new();
        response.clear();
        match self.buf_reader.read_line(&mut response) {
            Ok(_) => {}
            Err(e) => {
                panic!("{e:?}");
            }
        };
        //println!("Read from TCP: {}", response);
        match Self::get_last_word(&response) {
            Some(last_word) => {
                if !last_word.is_empty() {
                    let mut last_word_string = last_word.to_string();
                    //println!("Last word of response is {}", last_word_string);
                    Self::trim_newline(&mut last_word_string);
                    match last_word_string.parse::<f32>() {
                        Ok(c) => c,
                        Err(e) => {
                            panic!("TcpCommunication: Failed to convert response ({last_word}) to f32: {e:?}");
                        }
                    }
                } else {
                    panic!("TcpCommunication: Last word of response is empty.");
                }
            }
            None => {
                panic!("TcpCommunication: Response does not contain any words.");
            }
        }
    }

    pub fn execute(
        &mut self,
        rx_from_controllino_opt: Option<Receiver<InternalCommand>>,
        rx_from_gpio_opt: Option<Receiver<InternalCommand>>,
        tx_to_gpio_opt: Option<Sender<f32>>,
    ) {
        let spin_sleeper = SpinSleeper::default();
        let sleep_interval_millis = 10;
        let sleep_duration_ten_milli_sec = Duration::from_millis(sleep_interval_millis);
        loop {
            // *** communication with GpioHandler - response required ***
            match rx_from_gpio_opt {
                Some(ref rx_from_gpio) => {
                    match rx_from_gpio.try_recv() {
                        Ok(c) => {
                            match c {
                                InternalCommand::RequestSignal(s) => {
                                    let response_value = self.get_signal(s);
                                    match tx_to_gpio_opt {
                                        Some(ref tx_to_gpio) => match tx_to_gpio.send(response_value) {
                                            Ok(()) => {}
                                            Err(e) => {
                                                println!(
                                                            "Error in trying to send response value back to data logger: {e:?}"
                                                        );
                                            }
                                        },
                                        None => {
                                            panic!("TCP communication: Missing channel to return value to GpioHandler.");
                                        }
                                    }
                                }
                                _ => { /* Do nothing for all other commands */ }
                            }
                        }
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!("TcpCommunication: Error in trying to receive command from data logger: {e:?}");
                                }
                            }
                        }
                    }
                }
                None => {
                    // no value provided - do nothing
                }
            }
            // **********************************

            // *** communication with Controllino - no response required ***
            match rx_from_controllino_opt {
                Some(ref rx_from_controllino) => {
                    match rx_from_controllino.try_recv() {
                        Ok(c) => match c {
                            InternalCommand::SetRelay(c) => self.change_relay(c, true),
                            InternalCommand::UnsetRelay(c) => self.change_relay(c, false),
                            _ => { /* ignore all other commands */ },
                        },
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!("TcpCommunication: Error in trying to receive command from data logger: {e:?}");
                                }
                            }
                        }
                    }
                }
                None => {
                    // no value provided - do nothing
                }
            }
            // **********************************
            
            spin_sleeper.sleep(sleep_duration_ten_milli_sec);
        }
    }
}
