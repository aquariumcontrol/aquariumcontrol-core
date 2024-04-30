/// This enum is used for internal thread messaging from control circuits to Controllino
/// These values are nested into enum InternalCommand. They represent all actuation devices.
#[derive(Clone, Debug)]
pub enum AquariumDevice {
    /*
    Skimmer,
    MainPump1,
    MainPump2,
    AuxPump1,
    AuxPump2,
    Heater,
    Ventilation,
    */
    //RefillPump,
    /*
    Feeder,
    PeristalticPump1,
    PeristalticPump2,
    PeristalticPump3,
    PeristalticPump4,
    */
}

/// This enum is used for internal thread messaging between data logger and GPIO handler
/// The values indicate which information the GPIO handler thread shall return via channel.
#[derive(Clone, Debug)]
pub enum AquariumSignal {
    WaterTemperature,
    #[allow(non_camel_case_types)]
    pH,
    Conductivity,
    AmbientTemperature,
    AmbientHumidity,
    TankLevelSwitchPosition,
    TankLevelSwitchInvalid,
}

/// This enum is used for internal thread messaging
#[derive(Clone, Debug)]
pub enum InternalCommand {
    //SwitchOn(AquariumDevice),
    //SwitchOff(AquariumDevice),
    RequestSignal(AquariumSignal),
    //Quit,
}
