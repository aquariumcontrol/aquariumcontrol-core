# Water Software Requirements

### Read Pre-Processed Level Switch Position
`sw_req~water_read_level_switch~1`

The refill controller shall use the pre-processed tank level switch signals (`tank_level_switch_position_stabilized` and `tank_level_switch_invalid`).

Covers: sys_req~water1~1
Needs: impl, utest

### Determine Refill Trigger
`sw_req~water_determine_refill~1`

The controller shall initiate a fresh water refill if the stabilized switch position is LOW, the sensor status is valid, and no active error states or schedule inhibitions are present.

Covers: sys_req~water2~1
Needs: impl, utest

### Mutex Actuation Protection
`sw_req~water_actuate_pump~1`

The refill controller shall use the shared mutex (`mutex_device_scheduler_refill`) to serialize relay state changes and avoid parallel actuation of conflicting device relays when starting or stopping the refill pump.

Covers: sys_req~water3~1
Needs: impl, utest

### Hysteresis (Additional Refill Volume)
`sw_req~water_hysteresis~1`

The refill controller shall implement a hysteresis functionality. Once the pre-processed level switch signals HIGH (sump full), the controller shall continue to actuate the pump for an additional duration corresponding to the calibrated `additional_refill_volume` before turning off the pump.

Covers: sys_req~water3~1
Needs: impl, utest

### Log Refill Operation
`sw_req~water_log_database~1`

The control system shall log each refill sequence by creating a record in the database `refill` table, capturing start/stop timestamps, duration, calculated refill volume (based on duration and pump flow rate), and the system error code.

Covers: sys_req~water4~1
Needs: impl, utest

### Error Detection (Stuck-High and Stuck-Low)
`sw_req~water_error_detection~1`

The controller shall identify and raise errors for:
- **Stuck-High**: The level switch has not indicated a low level (remains HIGH) for an extended period of time exceeding `max_duration_since_last_refill`.
- **Stuck-Low**: The level switch fails to indicate a high level (remains LOW) during an active refill operation within a maximum pump runtime timeout based on a target maximum water volume for the refill operation.

Covers: sys_req~water5~1
Needs: impl, utest

### Faulty Sensor Protection (Volume & Count Limits)
`sw_req~water_faulty_switch_protection~1`

The controller shall protect against overfilling by monitoring and enforcing the following limits:
- Maximum permitted refill volume and maximum refill count within the last 24 hours (`max_refill_volume_24h` and `max_refill_count_24h`).
- Maximum permitted refill volume and maximum refill count within the last hour (`max_refill_volume_hour` and `max_refill_count_hour`).
If any of these limits are exceeded, subsequent refill operations shall be inhibited.

Covers: sys_req~water9~1
Needs: impl, utest

### Startup Data Recovery
`sw_req~water_startup_data_recovery~1`

Upon application startup, the controller shall read historical refill logs from the database to initialize the 1-hour and 24-hour volume and count statistics, preventing the unintentional reset of error states across system reboots.

Covers: sys_req~water5~1
Needs: impl, utest

### Calibration Parameters
`sw_req~water_calibration_parameters~1`

The system shall allow the user to define physical pump flow and volume limits in the configuration TOML file:
- Refill pump flow rate (`refill_pump_flow` in liters per second).
- Maximum allowed single refill volume (`max_refill_volume` in Liters).
- Safety limit thresholds (`max_refill_volume_hour`, `max_refill_volume_24h`, `max_refill_count_hour`, `max_refill_count_24h`).

Covers: sys_req~water9~1
Needs: impl, utest

### Log Flood Protection
`sw_req~water_log_flood_protection~1`

The controller shall use log warning suppression flags (such as `volume_last_24h`, `count_last_24h`, `volume_last_hour`, `count_last_hour` inside `RefillLocks` and `lock_warn_max_mutex_access_duration` inside `WaterInjection`) to suppress repetitive log warnings and prevent log flood.

Covers: sys_req~water5~1
Needs: impl, utest

### Shutdown Sequence
`sw_req~water_shutdown_sequence~1`

The controller shall handle OS shutdown requests received via the message channel from the signal handler:
- Upon receiving a `Quit` signal, it shall immediately stop the refill pump if it is active and exit the inner loop for monitoring the tank level switch and poll for further signals.
- Upon receiving a `Terminate` signal, it shall exit the thread context cleanly.

Covers: sys_req~water8~1
Needs: impl, utest

### Channel Communication
`sw_req~water_channel_communication~1`

The refill controller shall communicate with other threads using MPSC queues:
- **relay_manager**: to send pump relay commands and receive state feedback.
- **signal_handler**: to receive `Quit`/`Terminate` signals and return completion acknowledgments.
- **schedule_check**: to query allowed operational time windows.
- **messaging**: to receive remote user control overrides (`Start`, `Stop`, `ResetAllErrors`).
- **monitors**: to send diagnostic status views (`RefillMonitorView`).

Covers: sys_req~water6~1, sys_req~water7~1, sys_req~water8~1
Needs: impl, utest
