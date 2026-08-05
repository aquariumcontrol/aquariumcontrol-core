### [simulator] Request Simulated Sensor Signals
`swreq~simulator_sensor_signals~1`

The TCP communication component shall connect to an external TCP simulator and request simulated sensor signal values (`WaterTemperature`, `pH`, `Conductivity`, `AmbientTemperature`, `AmbientHumidity`, `TankLevelSwitchPosition`, `TankLevelSwitchInvalid`) over TCP, returning parsed floating-point values to requesting sensor modules (`SensorManager`, `TankLevelSwitch`).

Covers:

* sysreq~simulator1~1
Needs: impl, utest

### [simulator] Set Simulated Relay Actuator States
`swreq~simulator_relay_actuation~1`

The TCP communication component shall transmit relay actuation commands (`SetRelay`, `UnsetRelay`) to the external TCP simulator to switch simulated relays ON or OFF.

Covers:

* sysreq~simulator2~1
Needs: impl, utest
