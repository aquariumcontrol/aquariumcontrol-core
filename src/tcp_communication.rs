use std::io;
use std::{
    net::TcpStream,
    io::{prelude::*, BufReader}};
use serde_derive::Deserialize;

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
    tcp_connection_config: TcpConnectionConfig,
    stream: TcpStream,
}

impl TcpCommunication {
    pub fn new(tcp_connection: TcpConnectionConfig) -> Result<TcpCommunication, TcpCommunicationError>  {
        let mut stream = match TcpStream::connect(tcp_connection.ip_address.clone() + ":" + &tcp_connection.port.to_string()) {
            Ok(c) => c,
            Err(e) => {
                println!("Error when trying to connect to {} via port {}: {e:?}", tcp_connection.ip_address, tcp_connection.port);
                return Err(TcpCommunicationError::ConnectionFailed(e));
            }
        };
        let mut buf_reader = BufReader::new(&stream);
        let request = category::ATLASSCIENTIFIC.to_string() + " " + command::READ + " " + signal::ATLASSCIENTIFICTEMPERATURE + "\n";
        
        println!("Writing...");
        let mut write_stream = stream.try_clone().unwrap();
        match write_stream.write(request.as_bytes()) {
            Ok(_) => { },
            Err(e) => {panic!("{e:?}");}
        };
        let _ = write_stream.flush();
        
        println!("Reading...");
        let mut response = String::new();
        response.clear();
        match buf_reader.read_line(&mut response) {
            Ok(_) => {},
            Err(e) => {panic!("{e:?}");}
        };
        println!("Read from TCP: {}", response);
        
        Ok(TcpCommunication {
            tcp_connection_config: tcp_connection,
            stream: stream,
        })
    }
}