### [mineral] Read Balling Profiles
`swreq~mineral_read_profiles~1`

The control system shall query the database at check intervals to retrieve dosing pump configuration limits, calibration parameters, and daily dosing quantities.

Covers:

* sysreq~mineral1~1
Needs: impl, itest

### [mineral] Compute Next Mineral Injection Due Time
`swreq~mineral_compute_due~1`

The controller shall read the historic dosing logs and determine when the next dosing pump injection is due based on the daily target dose and dosing frequency.

Covers:

* sysreq~mineral2~1
Needs: impl, itest

### [mineral] Calculate Dosing Injection Duration
`swreq~mineral_calc_duration~1`

The controller shall convert target dosing volumes to active relay runtime seconds using each pump's calibrated flow rate constants.

Covers:

* sysreq~mineral3~1
Needs: impl, utest, itest

### [mineral] Log Mineral Dosing Operation
`swreq~mineral_log_database~1`

The control system shall write records to the database dosing log table containing pump ID, volume dosed, execution time, and status.

Covers:

* sysreq~mineral4~1
Needs: impl, itest

### [mineral] Execute Manual Dosing
`swreq~mineral_manual_dosing~1`

The controller shall parse manual dosing requests targeting specific pumps and actuate them for the requested duration immediately.

Covers:

* sysreq~mineral5~1
Needs: impl, itest

### [mineral] Stop Mineral Dosing Indefinitely
`swreq~mineral_stop_indefinitely~1`

The controller shall cancel active dosing pump operations and suspend automatic dosing cycles upon receiving a `Stop` command.

Covers:

* sysreq~mineral6~1
Needs: impl, itest

### [mineral] Connect to database
`swreq~mineral_connect_database~1`

The software shall connect to the database and initialize the SQL interface for mineral dosing parameters, verifying row count limits.

Covers:

* sysreq~mineral1~1
Needs: impl, itest


### [mineral] Restart Mineral Dosing
`swreq~mineral_restart~1`

The controller shall resume automatic mineral dosing cycles upon receiving a `Start` command.

Covers:

* sysreq~mineral7~1
Needs: impl, itest

### [mineral] Observe Operational Time Window
`swreq~mineral_observe_schedule~1`

The controller shall monitor the schedule check inhibition status and prevent or suspend mineral dosing operations if the current daytime is outside of the configured allowed operational window.

Covers:

* sysreq~schedule3~1
  Needs: impl, itest
