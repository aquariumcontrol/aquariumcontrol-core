# Mineral Software Requirements

### Read Balling Profiles
`sw_req~mineral_read_profiles~1`

The control system shall query the database at check intervals to retrieve dosing pump configuration limits, calibration parameters, and daily dosing quantities.

Covers: sys_req~mineral1~1
Needs: impl, utest

### Compute Next Mineral Injection Due Time
`sw_req~mineral_compute_due~1`

The controller shall read the historic dosing logs and determine when the next dosing pump injection is due based on the daily target dose and dosing frequency.

Covers: sys_req~mineral2~1
Needs: impl, utest

### Calculate Dosing Injection Duration
`sw_req~mineral_calc_duration~1`

The controller shall convert target dosing volumes to active relay runtime seconds using each pump's calibrated flow rate constants.

Covers: sys_req~mineral3~1
Needs: impl, utest

### Log Mineral Dosing Operation
`sw_req~mineral_log_database~1`

The control system shall write records to the database dosing log table containing pump ID, volume dosed, execution time, and status.

Covers: sys_req~mineral4~1
Needs: impl, utest

### Execute Manual Dosing
`sw_req~mineral_manual_dosing~1`

The controller shall parse manual dosing requests targeting specific pumps and actuate them for the requested duration immediately.

Covers: sys_req~mineral5~1
Needs: impl, utest

### Stop Mineral Dosing Indefinitely
`sw_req~mineral_stop_indefinitely~1`

The controller shall cancel active dosing pump operations and suspend automatic dosing cycles upon receiving a `Stop` command.

Covers: sys_req~mineral6~1
Needs: impl, utest

### Restart Mineral Dosing
`sw_req~mineral_restart~1`

The controller shall resume automatic mineral dosing cycles upon receiving a `Start` command.

Covers: sys_req~mineral7~1
Needs: impl, utest
