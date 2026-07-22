# Schedule Software Requirements

### Read Schedule Entries
`sw_req~schedule_read_entries~1`

The control system shall query the `schedule` database table at regular intervals to load allowed operational windows (start time of day, stop time of day) for each actuator category.

Covers: sys_req~schedule1~1
Needs: impl, utest

### Inhibit Refill Operations
`sw_req~schedule_inhibit_refill~1`

The schedule check controller shall evaluate the current local time of day against the refill operational window, raising an inhibition flag to the refill subsystem if the current time is outside the allowed window.

Covers: sys_req~schedule2~1
Needs: impl, utest

### Inhibit Balling Dosing Operations
`sw_req~schedule_inhibit_mineral~1`

The schedule check controller shall evaluate local time against the mineral dosing window, raising an inhibition flag to the dosing loop if the current time is outside the allowed window.

Covers: sys_req~schedule3~1
Needs: impl, utest

### Inhibit Heating Operations
`sw_req~schedule_inhibit_heating~1`

The schedule check controller shall evaluate local time against the heating operational window, raising an inhibition flag to the heating loop if the current time is outside the allowed window.

Covers: sys_req~schedule4~1
Needs: impl, utest

### Inhibit Ventilation Operations
`sw_req~schedule_inhibit_ventilation~1`

The schedule check controller shall evaluate local time against the ventilation operational window, raising an inhibition flag to the ventilation loop if the current time is outside the allowed window.

Covers: sys_req~schedule5~1
Needs: impl, utest
