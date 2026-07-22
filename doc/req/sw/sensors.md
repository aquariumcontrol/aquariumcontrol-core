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
