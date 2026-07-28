### [dispatch] Process POSIX Queue Messages
`swreq~dispatch_process_posix_messages~1`

The messaging module shall read incoming POSIX messages from the operating system message queue and route commands to target subsystems (refill, heater, feeder) using internal channel dispatchers.

Covers:

* sysreq~messaging~1
Needs: impl, itest

### [dispatch] Log Flood Protection
`swreq~dispatch_log_flood_protection~1`

The controller shall use log suppression flags to record warnings and errors and inhibit repeated identical logging.

Covers:

* sysreq~general1~1
Needs: impl, utest

