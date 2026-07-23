# Sensors Software Requirements

### Read Sensor Data
`sw_req~sensors_read_data~1`

The control system shall read measurements from configured sensor hardware:
- Water temperature and ambient parameters using 1-wire DS18B20 drivers.
- Ambient temperature/humidity from DHT22 GPIO line pulses.
- Water temperature, pH, and conductivity via I2C communications to Atlas Scientific EZO/OEM chips.
- Tank level state from physical digital level switch inputs.

Covers: sys-req~sensors1~1
Needs: impl, utest

### Tank Level Switch Signal Preprocessing
`sw_req~sensors_tank_level_switch_preprocessing~1`

The sensor manager shall perform preprocessing on the raw digital input from the tank level switch GPIO pin:
- Apply a stabilization/debounce filter counting consecutive matching readings (configured via `tank_level_switch_position_low_stabilization_count`).
- Publish a pre-processed boolean flag `tank_level_switch_position_stabilized` representing the stabilized switch position.
- Publish a pre-processed boolean flag `tank_level_switch_invalid` representing the signal invalidity status in case of signal interface failures.

Covers: sys-req~sensors1~1
Needs: impl, utest
