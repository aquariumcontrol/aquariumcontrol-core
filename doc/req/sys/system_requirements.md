# Water
### Read Tank Level Switch Position
[sys_req~water1~1]

The control system shall read the tank level switch position.
//Needs: sw_req~water_read_level_switch~1

### Determine Refill Trigger
[sys_req~water2~1]

The control system shall use the tank level switch position to determine when to refill fresh water.
Needs: sw_req~water_determine_refill~1

### Actuate Fresh Water Refill Pump
`sys_req~water3~1`

The control system shall actuate the fresh water pump to refill the main tank.
Needs: sw_req~water_actuate_pump~1, sw_req~water_hysteresis~1

### Log Refill Operation in Database
`sys_req~water4~1`

The control system shall log each fresh water refill operation in the database.
Needs: sw_req~water_log_database~1

### Recognize Subsystem Error States
`sys_req~water5~1`

The control system shall recognize error states of database communication, sensor, actuator or main tank.
Needs: sw_req~water_error_detection~1, sw_req~water_startup_data_recovery~1, sw_req~water_log_flood_protection~1, sw_req~water_db_failure_inhibition~1

### Reset Refill Error States Remotely
`sys_req~water6~1`

The control system shall allow the user to reset the error remotely.
Needs: sw_req~water_remote_reset~1, sw_req~water_channel_communication~1

### Start Refill Controller Remotely
`sys_req~water7~1`

The control system shall allow the user to start the refill controller remotely.
Needs: sw_req~water_remote_start~1, sw_req~water_channel_communication~1

### Stop Refill Controller Remotely
`sys_req~water8~1`

The control system shall allow the user to stop the refill controller remotely.
Needs: sw_req~water_remote_stop~1, sw_req~water_shutdown_sequence~1, sw_req~water_channel_communication~1

### Protect Against Faulty Level Switch
`sys_req~water9~1`

The control system shall protect against faulty tank level switch sensor causing excessive fresh water refill.
Needs: sw_req~water_faulty_switch_protection~1, sw_req~water_calibration_parameters~1

# Food
### Read Feed Schedule Entries
`sys_req~food1~1`

The control system shall repeatedly read feed schedule entries from the database.
The feed schedule entry defines when a certain feed profile shall be executed and with which frequency it shall be repeated.
Needs: sw_req~food_read_schedule~1

### Read Feed Profiles
`sys_req~food2~1`

The control system shall read the feed profiles from the database.
The feed profile consists of ten phases where each phase consists of a pause and a feed subphase.
For each subphase, the feed profile maintains the information which devices shall be ON or OFF.
At the end of the feed profile, the feeder shall be switched off and all other devices shall be switched on.
Needs: sw_req~food_read_profiles~1

### Execute Feed Profile per Schedule
`sys_req~food3~1`

The control system shall execute the feed profile as per feed schedule entry.
Needs: sw_req~food_execute_schedule~1

### Execute Feed Profile per Remote Request
`sys_req~food4~1`

The control system shall execute the feed profile as per remote user request.
Needs: sw_req~food_execute_remote~1

### Stop Feed Operations Indefinitely
`sys_req~food5~1`

The control system shall allow the user to stop all feed operations indefinitely.
Needs: sw_req~food_stop_indefinitely~1

### Restart Feed Operations
`sys_req~food6~1`

The control system shall allow the user to restart all feed operations.
Needs: sw_req~food_restart~1

### Log Feed Operation in Database
`sys_req~food7~1`

The control system shall log any feed operation in the database.
Needs: sw_req~food_log_database~1

# Mineral
### Read Balling Dosing Profiles
`sys_req~mineral1~1`

The control system shall repeatedly read the balling pump profiles from the database.
Needs: sw_req~mineral_read_profiles~1

### Compute Next Mineral Injection Due Time
`sys_req~mineral2~1`

The control system shall compute when the next mineral injection is due based on the log of previous mineral injections.
Needs: sw_req~mineral_compute_due~1

### Calculate Injection Duration
`sys_req~mineral3~1`

The control system shall calculate the injection duration of each balling pump based on the volume flow of each pump and the desired mineral dosing quantity.
Needs: sw_req~mineral_calc_duration~1

### Log Mineral Dosing Operation in Database
`sys_req~mineral4~1`

The control system shall log any mineral dosing operation in the database.
Needs: sw_req~mineral_log_database~1

### Execute Manual Mineral Dosing
`sys_req~mineral5~1`

The control system shall allow the user to execute a mineral dosing for a specific pump.
Needs: sw_req~mineral_manual_dosing~1

### Stop Mineral Dosing Indefinitely
`sys_req~mineral6~1`

The control system shall allow the to stop all mineral dosing operations indefinitely.
Needs: sw_req~mineral_stop_indefinitely~1

### Restart Mineral Dosing
`sys_req~mineral7~1`

The control system shall allow the to restart all mineral dosing operations indefinitely.
Needs: sw_req~mineral_restart~1

# Schedule
### Read Actuator Schedule Entries
`sys_req~schedule1~1`

The control system shall repeatedly read the actuator schedule entries from the database.
Each actuator schedule entry has a starting time of day and a stop time of day determining the allowed operation window for each actuator.
The database may contain schedule entries for the following actuators: mineral dosing, fresh water refill, ventilation, heating.
Needs: sw_req~schedule_read_entries~1

### Inhibit Refill Operations Outside Operational Window
`sys_req~schedule2~1`

The control system shall inhibit fresh water refill operations if the current daytime is outside of the operation window for the actuator.
Needs: sw_req~schedule_inhibit_refill~1, sw_req~water_observe_schedule~1

### Inhibit Balling Dosing Outside Operational Window
`sys_req~schedule3~1`

The control system shall inhibit balling mineral dosing if the current daytime is outside of the operation window for the actuator.
Needs: sw_req~schedule_inhibit_mineral~1

### Inhibit Heating Operations Outside Operational Window
`sys_req~schedule4~1`

The control system shall inhibit the use of the heater if the current daytime is outside of the operation window for the actuator.
Needs: sw_req~schedule_inhibit_heating~1

### Inhibit Ventilation Operations Outside Operational Window
`sys_req~schedule5~1`

The control system shall inhibit the use of the ventilation if the current daytime is outside of the operation window for the actuator.
Needs: sw_req~schedule_inhibit_ventilation~1

# Relays
### Control Aquarium System Actuators
`sys-req~relay1~1`

The control system shall control the following actuators:
- Protein skimmer
- Main pump #1
- Main pump #2
- Auxiliary pump #1
- Auxiliary pump #2
- Heater
- Feeder
Needs: sw_req~relay_control_actuators~1

# Sensors
### Read Aquarium Sensors
`sys-req~sensors1~1`

The control system read from the following sensors:
- Atlas Scientific temperature sensor for water temperature
- Atlas Scientific pH sensor
- Atlas Scientific conductivity sensor
- DHT11 ambient temperature and humidity sensor
- DS18B20 temperature sensor for water temperature
- DS18B20 temperature sensor for ambient temperature
- Tank level switch
Needs: sw_req~sensors_read_data~1, sw_req~sensors_tank_level_switch_preprocessing~1

# Thermal
### Reduce Water Temperature via Ventilation
`sys-req~thermal1~1`

The control system shall reduce the water temperature by actuating ventilation fans depending on the actual water temperature.
Needs: sw_req~thermal_ventilation_cooling~1

### Read Ventilation Target Temperature Window
`sys-req~thermal2~1`

The control system shall repeatedly read the target temperature window for the ventilation control from the database.
Needs: sw_req~thermal_read_ventilation_limits~1

### Stop Ventilation Control Remotely
`sys-req~thermal3~1`

The control system shall allow the user to stop the ventilation control.
Needs: sw_req~thermal_remote_ventilation_stop~1

### Restart Ventilation Control Remotely
`sys-req~thermal4~1`

The control system shall allow the user to re-start the ventilation control.
Needs: sw_req~thermal_remote_ventilation_start~1

### Increase Water Temperature via Heater
`sys-req~thermal5~1`

The control system shall increase the water temperature by actuating a heater depending on the actual water temperature.
Needs: sw_req~thermal_heater_heating~1

### Read Heating Target Temperature Window
`sys-req~thermal6~1`

The control system shall repeatedly read the target temperature window for the heating control from the database.
Needs: sw_req~thermal_read_heating_limits~1

### Stop Heating Control Remotely
`sys-req~thermal7~1`

The control system shall allow the user to stop the heating control.
Needs: sw_req~thermal_remote_heating_stop~1

### Restart Heating Control Remotely
`sys-req~thermal8~1`

The control system shall allow the user to re-start the heating control.
Needs: sw_req~thermal_remote_heating_start~1

### Inhibit Heater on Low Sump Water Level
`sys_req~thermal9~1`

The control system shall inhibit the heater if the tank level switch reports a low water level for an extended time period.
Needs: sw_req~thermal_inhibit_heating_low_water~1

# Watchmen
### Self Monitoring and Liveness
`sys-req~watchmen~1`

The control system shall monitor itself.
Needs: sw_req~watchmen_self_monitoring~1

### Pet Hardware Watchdog
`sys_req~watchmen2~1`

The control system shall pet the hardware watchdog at regular intervals to verify process liveness.
Needs: sw_req~watchmen_pet_watchdog~1

### Detect Execution Stalls and Trigger Reboot
`sys_req~watchmen3~1`

The control system shall detect execution stalls or component failures and signal the watchdog to trigger a safe system reboot.
Needs: sw_req~watchmen_reboot_on_stall~1

# Recorder
### Record Measurement Signals in Database
`sys-req~recorder~1`

The control system shall regularly record measurement signals in the database.
Needs: sw_req~recorder_store_measurements~1

### Filter Raw Sensor Readings
`sys_req~recorder2~1`

The control system shall filter raw sensor readings using an infinite impulse response (IIR) filter to eliminate measurement noise.
Needs: sw_req~recorder_apply_iir_filter~1

# Utilities
### Process OS Shutdown Signals
`sys-req~utilities1~1`

The control system shall receive signals from the operating system and react accordingly.
Needs: sw_req~utilities_process_signals~1

### Read Configuration File on Startup
`sys_req~utilities2~1`

The control system shall read all configuration parameters from a local configuration file on startup.
Needs: sw_req~utilities_parse_toml_config~1

### Verify Version Hash Compatibility
`sys_req~utilities3~1`

The control system shall verify that its internal version information and hash are compatible with the database schema on startup.
Needs: sw_req~utilities_check_version_compatibility~1

### Publish Process ID File
`sys_req~utilities4~1`

The control system shall publish its process ID (PID) to a file on startup to prevent duplicate running instances.
Needs: sw_req~utilities_write_pid_file~1

# Dispatch
### Receive POSIX Queue Messages
`sys-req~messaging~1`

The control system shall receive POSIX messages from the operating system and react accordingly.
Needs: sw_req~dispatch_process_posix_messages~1
