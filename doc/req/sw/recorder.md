### [recorder] Store Measurement Signals
`swreq~recorder_store_measurements~1`

The data logger subsystem shall periodically compile system measurements (temperatures, pH, conductivity, humidity, actuator states) and commit them to the database log tables.

Covers:

* sysreq~recorder~1
Needs: impl, utest

### [recorder] Apply IIR Filtering
`swreq~recorder_apply_iir_filter~1`

The system shall filter raw sensor inputs using an Infinite Impulse Response (IIR) filter configured with smoothing coefficients to mitigate high-frequency sensor noise.

Covers:

* sysreq~recorder2~1
Needs: impl, utest
