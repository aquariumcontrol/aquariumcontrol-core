### [food] Read Feed Schedules
`swreq~food_read_schedule~1`

The control system shall query the `feed_schedule` database table at regular intervals to load active feeding times, repetition frequencies, and linked profile IDs.

Covers:

* sysreq~food1~1
Needs: impl, utest, itest

### [food] Read Feed Profiles
`swreq~food_read_profiles~1`

The control system shall query the `feed_profile` database table to load configured feeding sequences, each consisting of up to ten phases of pause and feed durations, specifying device ON/OFF states.

Covers:

* sysreq~food2~1
Needs: impl, itest

### [food] Execute Feed Schedule
`swreq~food_execute_schedule~1`

The controller shall monitor local time against scheduled feeding entry times, and actuate the feeder and peripheral relays (such as turning off skimmer/pumps) according to the active profile phases.

Covers:

* sysreq~food3~1
Needs: impl, utest, itest

### [food] Execute Remote Feed Request
`swreq~food_execute_remote~1`

The controller shall parse immediate manual feed requests received via the OS message queues and initiate feed profile execution immediately.

Covers:

* sysreq~food4~1
Needs: impl, itest

### [food] Remote Feed Stop
`swreq~food_stop_indefinitely~1`

The controller shall abort any active feed operation and suspend scheduled feedings indefinitely upon receiving a `Stop` command.

Covers:

* sysreq~food5~1
Needs: impl, itest

### [food] Remote Feed Restart
`swreq~food_restart~1`

The controller shall resume scheduled feeding cycles upon receiving a `Start` command.

Covers:

* sysreq~food6~1
Needs: impl, itest

### [food] Log Feed Operation
`swreq~food_log_database~1`

The controller shall log each feeding execution in the database log tables containing profile ID, time of execution, and status.

Covers:

* sysreq~food7~1
Needs: impl, utest, itest

### [food] Connect to database
`swreq~food_connect_database~1`

The software shall connect to the database and initialize the SQL interface for feed schedules and profiles, checking table row count limits.

Covers:

* sysreq~food1~1
Needs: impl, itest

### [food] Log Flood Protection
`swreq~food_log_flood_protection~1`

The controller shall use log suppression flags to record warnings and errors and inhibit repeated identical logging.

Covers:

* sysreq~general1~1
Needs: impl, utest



