### [schedule] Read Schedule Entries
`swreq~schedule_read_entries~1`

The control system shall query the `schedule` database table at regular intervals to load allowed operational windows (start time of day, stop time of day) for each actuator category.

Covers:

* sysreq~schedule1~1
Needs: impl, utest

### [schedule] Inhibit Refill Operations
`swreq~schedule_inhibit_refill~1`

The schedule check controller shall evaluate the current local time of day against the refill operational window, raising an inhibition flag to the refill subsystem if the current time is outside the allowed window.

Covers:

* sysreq~schedule2~1
Needs: impl, utest

### [schedule] Inhibit Balling Dosing Operations
`swreq~schedule_inhibit_mineral~1`

The schedule check controller shall evaluate local time against the mineral dosing window, raising an inhibition flag to the dosing loop if the current time is outside the allowed window.

Covers:

* sysreq~schedule3~1
Needs: impl, utest

### [schedule] Inhibit Heating Operations
`swreq~schedule_inhibit_heating~1`

The schedule check controller shall evaluate local time against the heating operational window, raising an inhibition flag to the heating loop if the current time is outside the allowed window.

Covers:

* sysreq~schedule4~1
Needs: impl, utest

### [schedule] Inhibit Ventilation Operations
`swreq~schedule_inhibit_ventilation~1`

The schedule check controller shall evaluate local time against the ventilation operational window, raising an inhibition flag to the ventilation loop if the current time is outside the allowed window.

Covers:

* sysreq~schedule5~1
Needs: impl, utest
