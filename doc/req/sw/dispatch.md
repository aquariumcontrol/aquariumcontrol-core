# Dispatch Software Requirements

### Process POSIX Queue Messages
`sw_req~dispatch_process_posix_messages~1`

The messaging module shall read incoming POSIX messages from the operating system message queue and route commands to target subsystems (refill, heater, feeder) using internal channel dispatchers.

Covers: sys-req~messaging~1
Needs: impl, utest
