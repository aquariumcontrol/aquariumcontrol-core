### [sensors] Read Sensor Data
`swreq~sensors_read_data~1`

The control system shall read measurements from configured sensor hardware:
- Water temperature and ambient parameters using 1-wire DS18B20 drivers.
- Ambient temperature/humidity from DHT22 GPIO line pulses.
- Water temperature, pH, and conductivity via I2C communications to Atlas Scientific EZO/OEM chips.
- Tank level state from physical digital level switch inputs.

Covers:

* sysreq~sensors1~1
Needs: impl, itest

### [sensors] Tank Level Switch Signal Preprocessing
`swreq~sensors_tank_level_switch_preprocessing~1`

The software shall perform preprocessing on the raw digital input from the tank level switch GPIO pin:
- Apply a stabilization/debounce filter counting consecutive matching readings (configured via `tank_level_switch_position_low_stabilization_count`).
- Publish a pre-processed boolean flag `tank_level_switch_position_stabilized` representing the stabilized switch position.
- Publish a pre-processed boolean flag `tank_level_switch_invalid` representing the signal invalidity status in case of signal interface failures.

Covers:

* sysreq~sensors1~1
Needs: impl, itest

### [sensors] Tank Level Switch Signal Invalid behavior
`swreq~sensors_tank_level_switch_invalid~1`

The software shall set the processed tank level switch signal directly to high if an invalid input signal is detected.

Covers:

* sysreq~sensors1~1
Needs: impl, itest


### [sensors] Log Flood Protection
`swreq~sensors_log_flood_protection~1`

The controller shall use log suppression flags to record warnings and errors and inhibit repeated identical logging.

Covers:

* sysreq~general1~1
Needs: impl, utest, itest


