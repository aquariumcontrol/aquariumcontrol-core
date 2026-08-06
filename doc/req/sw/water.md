### [water] Read Pre-Processed Level Switch Position
`swreq~water_read_level_switch~1`

The refill controller shall use the pre-processed tank level switch signals (`tank_level_switch_position_stabilized` and `tank_level_switch_invalid`).

Covers:

* sysreq~water1~1
Needs: impl, itest, utest

### [water] Connect to database
`swreq~water_connect_database~1`

The software shall connect to the database and initialize the SQL interface for water refill parameters, verifying row count limits and recovering initial refill state.

Covers:

* sysreq~water1~1
Needs: impl, itest


### [water] Determine Refill Trigger
`swreq~water_determine_refill~1`

The controller shall initiate a fresh water refill if the stabilized switch position is LOW, the sensor status is valid, and no active error states or schedule inhibitions are present.

Covers:

* sysreq~water2~1
Needs: impl, utest, itest

### [water] Database Failure Inhibition
`swreq~water_db_failure_inhibition~1`

The refill controller shall inhibit and lock out further fresh water refill operations if a database communication failure occurs (such as failing to read historical statistics or failing to write logs).

Covers:

* sysreq~water5~1
Needs: impl, itest

### [water] Observe Operational Time Window
`swreq~water_observe_schedule~2`

The refill controller shall monitor the schedule check inhibition status and prevent or suspend refill pump operations if the current daytime is outside the configured allowed operational window.

Covers:

* sysreq~schedule2~1
Needs: impl, itest

### [water] Mutex Actuation Protection
`swreq~water_actuate_pump~1`

The refill controller shall use the shared mutex (`mutex_device_scheduler_refill`) to serialize relay state changes and avoid parallel actuation of conflicting device relays when starting or stopping the refill pump.

Covers:

* sysreq~water3~1
Needs: impl, itest

### [water] Hysteresis (Additional Refill Volume)
`swreq~water_hysteresis~1`

The refill controller shall implement a hysteresis functionality. Once the pre-processed level switch signals HIGH (sump full), the controller shall continue to actuate the pump for an additional duration corresponding to the calibrated `additional_refill_volume` before turning off the pump.

Covers:

* sysreq~water3~1
Needs: impl, itest

### [water] Log Refill Operation
`swreq~water_log_database~1`

The control system shall log each refill sequence by creating a record in the database `refill` table, capturing start/stop timestamps, duration, calculated refill volume (based on duration and pump flow rate), and the system error code.

Covers:

* sysreq~water4~1
Needs: impl, itest

### [water] Error Detection (Stuck-High and Stuck-Low)
`swreq~water_error_detection~1`

The controller shall identify and raise errors for:
- **Stuck-High**: The level switch has not indicated a low level (remains HIGH) for an extended period of time exceeding `max_duration_since_last_refill`.
- **Stuck-Low**: The level switch fails to indicate a high level (remains LOW) during an active refill operation within a maximum pump runtime timeout based on a target maximum water volume for the refill operation.

Covers:

* sysreq~water5~1
Needs: impl, itest, utest

### [water] Faulty Sensor Protection (Volume & Count Limits)
`swreq~water_faulty_switch_protection~1`

The controller shall protect against overfilling by monitoring and enforcing the following limits:
- Maximum permitted refill volume and maximum refill count within the last 24 hours (`max_refill_volume_24h` and `max_refill_count_24h`).
- Maximum permitted refill volume and maximum refill count within the last hour (`max_refill_volume_hour` and `max_refill_count_hour`).
If any of these limits are exceeded, subsequent refill operations shall be inhibited.

Covers:

* sysreq~water9~1
Needs: impl, utest, itest

### [water] Startup Data Recovery
`swreq~water_startup_data_recovery~1`

Upon application startup, the controller shall read historical refill logs from the database to initialize the 1-hour and 24-hour volume and count statistics, preventing the unintentional reset of error states across system reboots.

Covers:

* sysreq~water5~1
Needs: impl, utest, itest

### [water] Calibration Parameters
`swreq~water_calibration_parameters~1`

The system shall allow the user to define physical pump flow and volume limits in the configuration TOML file:
- Refill pump flow rate (`refill_pump_flow` in liters per second).
- Maximum allowed single refill volume (`max_refill_volume` in Liters).
- Safety limit thresholds (`max_refill_volume_hour`, `max_refill_volume_24h`, `max_refill_count_hour`, `max_refill_count_24h`).

Covers:

* sysreq~water9~1
Needs: impl, utest

### [water] Log Flood Protection
`swreq~water_log_flood_protection~1`

The controller shall use log suppression flags to record warnings and errors and inhibit repeated identical logging.

Covers:

* sysreq~general1~1
Needs: impl, utest

### [water] Shutdown Sequence
`swreq~water_shutdown_sequence~1`

The controller shall handle OS shutdown requests received via the message channel from the signal handler:
- Upon receiving a `Quit` signal, it shall immediately stop the refill pump if it is active and exit the inner loop for monitoring the tank level switch and poll for further signals.
- Upon receiving a `Terminate` signal, it shall exit the thread context cleanly.

Covers:

* sysreq~water8~1
Needs: impl, itest

### [water] Channel Communication
`swreq~water_channel_communication~1`

The refill controller shall communicate with other threads using MPSC queues:
- **relay_manager**: to send pump relay commands and receive state feedback.
- **signal_handler**: to receive `Quit`/`Terminate` signals and return completion acknowledgments.
- **schedule_check**: to query allowed operational time windows.
- **messaging**: to receive remote user control overrides (`Start`, `Stop`, `ResetAllErrors`).
- **monitors**: to send diagnostic status views (`RefillMonitorView`).

Covers:

* sysreq~water6~1
* sysreq~water7~1
* sysreq~water8~1
Needs: impl, itest
