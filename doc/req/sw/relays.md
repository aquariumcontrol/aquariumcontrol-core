### [relays] Control Actuators
`swreq~relay_control_actuators~1`

The RelayManager component shall receive state commands (`SwitchOn` / `SwitchOff`) from different subsystems and map them to physical Controllino relays or digital GPIO pins.

Covers:

* sysreq~relay1~1
Needs: impl, itest

### [relays] Log Flood Protection
`swreq~relays_log_flood_protection~1`

The controller shall use log suppression flags to record warnings and errors and inhibit repeated identical logging.

Covers:

* sysreq~general1~1
Needs: impl, utest



