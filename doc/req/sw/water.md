# Water Software Requirements

### Read Tank Level Switch Position
`sw_req~water_read_level_switch~1`

The control system shall query the digital GPIO pin state configured via `gpio_handler.tank_level_switch` to determine the raw water level status.

Covers: sys_req~water1~1
Needs: impl, utest

### Determine Refill Trigger
`sw_req~water_determine_refill~1`

The controller shall evaluate the debounced level switch position at configured check intervals and initiate a fresh water refill if the stabilized position is LOW and no active system locks are present.

Covers: sys_req~water2~1
Needs: impl, utest

### Actuate Fresh Water Pump
`sw_req~water_actuate_pump~1`

The control system shall dispatch `SwitchOn(AquariumDevice::RefillPump)` and `SwitchOff(AquariumDevice::RefillPump)` commands to the relay controller via the internal messaging queue to cycle the pump.

Covers: sys_req~water3~1
Needs: impl, utest

### Log Refill Operation
`sw_req~water_log_database~1`

The control system shall log each refill sequence by creating a record in the `refill` database table containing start/stop timestamps and duration.

Covers: sys_req~water4~1
Needs: impl, utest

### Error Detection
`sw_req~water_error_detection~1`

The controller shall detect refill anomalies including stuck-high switches (pump active but level remains low) and stuck-low switches (pump off but level remains low for too long), setting corresponding error flags in `RefillErrorStates`.

Covers: sys_req~water5~1
Needs: impl, utest

### Remote Error Reset
`sw_req~water_remote_reset~1`

The controller shall reset all refill fault states upon receiving a `ResetAllErrors` command through the operating system message channel.

Covers: sys_req~water6~1
Needs: impl, utest

### Remote Controller Start
`sw_req~water_remote_start~1`

The controller shall resume automatic refill operation and accept manual refill overrides when receiving a remote `Start` command.

Covers: sys_req~water7~1
Needs: impl, utest

### Remote Controller Stop
`sw_req~water_remote_stop~1`

The controller shall immediately stop any active refill pump operation and suspend automatic cycles upon receiving a remote `Stop` command.

Covers: sys_req~water8~1
Needs: impl, utest

### Faulty Sensor Protection
`sw_req~water_faulty_switch_protection~1`

The controller shall enforce maximum refill execution durations (configured via `max_injection_duration_secs`) and raise a terminal timeout error if the limit is exceeded.

Covers: sys_req~water9~1
Needs: impl, utest
