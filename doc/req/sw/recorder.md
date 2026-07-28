### [recorder] Store Measurement Signals
`swreq~recorder_store_measurements~1`

The data logger subsystem shall periodically compile system measurements (temperatures, pH, conductivity, humidity, actuator states) and commit them to the database log tables.

Covers:

* sysreq~recorder~1
Needs: impl, utest, itest

### [recorder] Apply IIR Filtering
`swreq~recorder_apply_iir_filter~1`

The system shall filter raw sensor inputs using an Infinite Impulse Response (IIR) filter configured with smoothing coefficients to mitigate high-frequency sensor noise.

Covers:

* sysreq~recorder2~1
Needs: impl, utest

### [recorder] Connect to database
`swreq~recorder_connect_database~1`

The software shall connect to the database and check if the number of rows exceeds the configured limit.

Covers:

* sysreq~recorder~1
Needs: impl, itest

### [recorder] Log Flood Protection
`swreq~recorder_log_flood_protection~1`

The controller shall use log suppression flags to record warnings and errors and inhibit repeated identical logging.

Covers:

* sysreq~general1~1
Needs: impl, utest


