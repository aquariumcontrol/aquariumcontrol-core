# Recorder Software Requirements

### Store Measurement Signals
`sw_req~recorder_store_measurements~1`

The data logger subsystem shall periodically compile system measurements (temperatures, pH, conductivity, humidity, actuator states) and commit them to the database log tables.

Covers: sys-req~recorder~1
Needs: impl, utest

### Apply IIR Filtering
`sw_req~recorder_apply_iir_filter~1`

The system shall filter raw sensor inputs using an Infinite Impulse Response (IIR) filter configured with smoothing coefficients to mitigate high-frequency sensor noise.

Covers: sys_req~recorder2~1
Needs: impl, utest
