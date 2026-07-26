### [relays] Control Actuators
`swreq~relay_control_actuators~1`

The RelayManager component shall receive state commands (`SwitchOn` / `SwitchOff`) from different subsystems and map them to physical Controllino relays or digital GPIO pins.

Covers:

* sysreq~relay1~1
Needs: impl, utest, itest
