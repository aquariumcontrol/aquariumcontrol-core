### [thermal] Ventilation Cooling Control
`swreq~thermal_ventilation_cooling~1`

The ventilation controller shall actuate ventilation cooling fans using a two-point control loop depending on the actual water temperature setpoint thresholds.

Covers:

* sysreq~thermal1~1
Needs: impl, itest

### [thermal] Read Ventilation Limits
`swreq~thermal_read_ventilation_limits~1`

The controller shall repeatedly read ventilation temperature setpoint windows and hystereses from the database.

Covers:

* sysreq~thermal2~1
Needs: impl, itest

### [thermal] Stop Ventilation Control Remotely
`swreq~thermal_remote_ventilation_stop~1`

The controller shall immediately turn off ventilation fan relays and suspend automated cooling loops upon receiving a remote `Stop` command.

Covers:

* sysreq~thermal3~1
Needs: impl, itest

### [thermal] Restart Ventilation Control Remotely
`swreq~thermal_remote_ventilation_start~1`

The controller shall resume automated temperature cooling loops upon receiving a remote `Start` command.

Covers:

* sysreq~thermal4~1
Needs: impl, itest

### [thermal] Heater Heating Control
`swreq~thermal_heater_heating~1`

The heating controller shall actuate the heater relay using a two-point control loop depending on the actual water temperature setpoint thresholds.

Covers:

* sysreq~thermal5~1
Needs: impl, itest

### [thermal] Read Heating Limits
`swreq~thermal_read_heating_limits~1`

The controller shall repeatedly read heating temperature setpoint windows and hystereses from the database.

Covers:

* sysreq~thermal6~1
Needs: impl, itest

### [thermal] Stop Heating Control Remotely
`swreq~thermal_remote_heating_stop~1`

The controller shall immediately turn off the heater relay and suspend automated heating loops upon receiving a remote `Stop` command.

Covers:

* sysreq~thermal7~1
Needs: impl, itest

### [thermal] Restart Heating Control Remotely
`swreq~thermal_remote_heating_start~1`

The controller shall resume automated temperature heating loops upon receiving a remote `Start` command.

Covers:

* sysreq~thermal8~1
Needs: impl, itest

### [thermal] Inhibit Heater on Low Sump Water Level
`swreq~thermal_inhibit_heating_low_water~1`

The heating controller shall monitor the tank level switch and immediately disable the heater relay if a low water level is detected in the sump, preventing heater burnout.

Covers:

* sysreq~thermal9~1
Needs: impl, itest

### [thermal] Check Distinct Target Temperature Windows
`swreq~thermal_distinct_interval_check~1`

Upon startup, the control system shall verify that the target temperature windows for heating and ventilation do not overlap, aborting startup if an overlap is detected.

Covers:

* sysreq~thermal10~1
Needs: impl, utest

### [thermal] Connect to Database for Heating Statistics
`swreq~thermal_connect_heating_stats~1`

The software shall connect to the database and initialize the SQL interface for recording daily heating energy statistics, verifying table row count limits.

Covers:

* sysreq~thermal5~1
Needs: impl, itest

### [thermal] Connect to Database for Midnight Calculation
`swreq~thermal_midnight_calculator~1`

The software shall connect to the database and initialize the midnight calculator interface to determine duration until midnight for daily statistics calculation.

Covers:

* sysreq~thermal5~1
Needs: impl, itest

### [thermal] Observe Operational Time Window for heater
`swreq~thermal_observe_schedule_heater~1`

The controller shall monitor the schedule check inhibition status and suspend heater actuation if the current daytime is outside the configured allowed operational window.

Covers:

* sysreq~schedule4~1
Needs: impl, itest

### [thermal] Observe Operational Time Window for ventilation
`swreq~thermal_observe_schedule_ventilation~1`

The controller shall monitor the schedule check inhibition status and suspend ventilation actuation if the current daytime is outside the configured allowed operational window.

Covers:

* sysreq~schedule5~1
Needs: impl, itest

### [thermal] Log Flood Protection
`swreq~thermal_log_flood_protection~1`

The controller shall use log suppression flags to record warnings and errors and inhibit repeated identical logging.

Covers:

* sysreq~general1~1
Needs: impl, utest, itest



