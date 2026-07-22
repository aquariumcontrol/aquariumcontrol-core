# Food Software Requirements

### Read Feed Schedules
`sw_req~food_read_schedule~1`

The control system shall query the `feed_schedule` database table at regular intervals to load active feeding times, repetition frequencies, and linked profile IDs.

Covers: sys_req~food1~1
Needs: impl, utest

### Read Feed Profiles
`sw_req~food_read_profiles~1`

The control system shall query the `feed_profile` database table to load configured feeding sequences, each consisting of up to ten phases of pause and feed durations, specifying device ON/OFF states.

Covers: sys_req~food2~1
Needs: impl, utest

### Execute Feed Schedule
`sw_req~food_execute_schedule~1`

The controller shall monitor local time against scheduled feeding entry times, and actuate the feeder and peripheral relays (such as turning off skimmer/pumps) according to the active profile phases.

Covers: sys_req~food3~1
Needs: impl, utest

### Execute Remote Feed Request
`sw_req~food_execute_remote~1`

The controller shall parse immediate manual feed requests received via the OS message queues and initiate feed profile execution immediately.

Covers: sys_req~food4~1
Needs: impl, utest

### Remote Feed Stop
`sw_req~food_stop_indefinitely~1`

The controller shall abort any active feed operation and suspend scheduled feedings indefinitely upon receiving a `Stop` command.

Covers: sys_req~food5~1
Needs: impl, utest

### Remote Feed Restart
`sw_req~food_restart~1`

The controller shall resume scheduled feeding cycles upon receiving a `Start` command.

Covers: sys_req~food6~1
Needs: impl, utest

### Log Feed Operation
`sw_req~food_log_database~1`

The controller shall log each feeding execution in the database log tables containing profile ID, time of execution, and status.

Covers: sys_req~food7~1
Needs: impl, utest
