# Thermal Software Requirements

### Ventilation Cooling Control
`sw_req~thermal_ventilation_cooling~1`

The ventilation controller shall actuate ventilation cooling fans using a two-point control loop depending on the actual water temperature setpoint thresholds.

Covers: sys-req~thermal1~1
Needs: impl, utest

### Read Ventilation Limits
`sw_req~thermal_read_ventilation_limits~1`

The controller shall repeatedly read ventilation temperature setpoint windows and hystereses from the database.

Covers: sys-req~thermal2~1
Needs: impl, utest

### Stop Ventilation Control Remotely
`sw_req~thermal_remote_ventilation_stop~1`

The controller shall immediately turn off ventilation fan relays and suspend automated cooling loops upon receiving a remote `Stop` command.

Covers: sys-req~thermal3~1
Needs: impl, utest

### Restart Ventilation Control Remotely
`sw_req~thermal_remote_ventilation_start~1`

The controller shall resume automated temperature cooling loops upon receiving a remote `Start` command.

Covers: sys-req~thermal4~1
Needs: impl, utest

### Heater Heating Control
`sw_req~thermal_heater_heating~1`

The heating controller shall actuate the heater relay using a two-point control loop depending on the actual water temperature setpoint thresholds.

Covers: sys-req~thermal5~1
Needs: impl, utest

### Read Heating Limits
`sw_req~thermal_read_heating_limits~1`

The controller shall repeatedly read heating temperature setpoint windows and hystereses from the database.

Covers: sys-req~thermal6~1
Needs: impl, utest

### Stop Heating Control Remotely
`sw_req~thermal_remote_heating_stop~1`

The controller shall immediately turn off the heater relay and suspend automated heating loops upon receiving a remote `Stop` command.

Covers: sys-req~thermal7~1
Needs: impl, utest

### Restart Heating Control Remotely
`sw_req~thermal_remote_heating_start~1`

The controller shall resume automated temperature heating loops upon receiving a remote `Start` command.

Covers: sys-req~thermal8~1
Needs: impl, utest

### Inhibit Heater on Low Sump Water Level
`sw_req~thermal_inhibit_heating_low_water~1`

The heating controller shall monitor the tank level switch and immediately disable the heater relay if a low water level is detected in the sump, preventing heater burnout.

Covers: sys_req~thermal9~1
Needs: impl, utest
