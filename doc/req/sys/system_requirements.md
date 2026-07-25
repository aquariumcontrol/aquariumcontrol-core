## sys-water: Water
### Read Tank Level Switch Position
`sysreq~water1~1`

The control system shall read the tank level switch position.
Needs: swreq

### Determine Refill Trigger
`sysreq~water2~1`

The control system shall use the tank level switch position to determine when to refill fresh water.
Needs: swreq

### Actuate Fresh Water Refill Pump
`sysreq~water3~1`

The control system shall actuate the fresh water pump to refill the main tank.
Needs: swreq

### Log Refill Operation in Database
`sysreq~water4~1`

The control system shall log each fresh water refill operation in the database.
Needs: swreq

### Recognize Subsystem Error States
`sysreq~water5~1`

The control system shall recognize error states of database communication, sensor, actuator or main tank.
Needs: swreq

### Reset Refill Error States Remotely
`sysreq~water6~1`

The control system shall allow the user to reset the error remotely.
Needs: swreq

### Start Refill Controller Remotely
`sysreq~water7~1`

The control system shall allow the user to start the refill controller remotely.
Needs: swreq

### Stop Refill Controller Remotely
`sysreq~water8~1`

The control system shall allow the user to stop the refill controller remotely.
Needs: swreq

### Protect Against Faulty Level Switch
`sysreq~water9~1`

The control system shall protect against faulty tank level switch sensor causing excessive fresh water refill.
Needs: swreq

## sys-food: Food
### Read Feed Schedule Entries
`sysreq~food1~1`

The control system shall repeatedly read feed schedule entries from the database.
The feed schedule entry defines when a certain feed profile shall be executed and with which frequency it shall be repeated.
Needs: swreq

### Read Feed Profiles
`sysreq~food2~1`

The control system shall read the feed profiles from the database.
The feed profile consists of ten phases where each phase consists of a pause and a feed subphase.
For each subphase, the feed profile maintains the information which devices shall be ON or OFF.
At the end of the feed profile, the feeder shall be switched off and all other devices shall be switched on.
Needs: swreq

### Execute Feed Profile per Schedule
`sysreq~food3~1`

The control system shall execute the feed profile as per feed schedule entry.
Needs: swreq

### Execute Feed Profile per Remote Request
`sysreq~food4~1`

The control system shall execute the feed profile as per remote user request.
Needs: swreq

### Stop Feed Operations Indefinitely
`sysreq~food5~1`

The control system shall allow the user to stop all feed operations indefinitely.
Needs: swreq

### Restart Feed Operations
`sysreq~food6~1`

The control system shall allow the user to restart all feed operations.
Needs: swreq

### Log Feed Operation in Database
`sysreq~food7~1`

The control system shall log any feed operation in the database.
Needs: swreq

## sys-mineral: Mineral
### Read Balling Dosing Profiles
`sysreq~mineral1~1`

The control system shall repeatedly read the balling pump profiles from the database.
Needs: swreq

### Compute Next Mineral Injection Due Time
`sysreq~mineral2~1`

The control system shall compute when the next mineral injection is due based on the log of previous mineral injections.
Needs: swreq

### Calculate Injection Duration
`sysreq~mineral3~1`

The control system shall calculate the injection duration of each balling pump based on the volume flow of each pump and the desired mineral dosing quantity.
Needs: swreq

### Log Mineral Dosing Operation in Database
`sysreq~mineral4~1`

The control system shall log any mineral dosing operation in the database.
Needs: swreq

### Execute Manual Mineral Dosing
`sysreq~mineral5~1`

The control system shall allow the user to execute a mineral dosing for a specific pump.
Needs: swreq

### Stop Mineral Dosing Indefinitely
`sysreq~mineral6~1`

The control system shall allow the to stop all mineral dosing operations indefinitely.
Needs: swreq

### Restart Mineral Dosing
`sysreq~mineral7~1`

The control system shall allow the to restart all mineral dosing operations indefinitely.
Needs: swreq

## sys-schedule: Schedule
### Read Actuator Schedule Entries
`sysreq~schedule1~1`

The control system shall repeatedly read the actuator schedule entries from the database.
Each actuator schedule entry has a starting time of day and a stop time of day determining the allowed operation window for each actuator.
The database may contain schedule entries for the following actuators: mineral dosing, fresh water refill, ventilation, heating.
Needs: swreq

### Inhibit Refill Operations Outside Operational Window
`sysreq~schedule2~1`

The control system shall inhibit fresh water refill operations if the current daytime is outside of the operation window for the actuator.
Needs: swreq

### Inhibit Balling Dosing Outside Operational Window
`sysreq~schedule3~1`

The control system shall inhibit balling mineral dosing if the current daytime is outside of the operation window for the actuator.
Needs: swreq

### Inhibit Heating Operations Outside Operational Window
`sysreq~schedule4~1`

The control system shall inhibit the use of the heater if the current daytime is outside of the operation window for the actuator.
Needs: swreq

### Inhibit Ventilation Operations Outside Operational Window
`sysreq~schedule5~1`

The control system shall inhibit the use of the ventilation if the current daytime is outside of the operation window for the actuator.
Needs: swreq

## sys-relays: Relays
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
Needs: swreq

## sys-sensors: Sensors
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
Needs: swreq

## sys-thermal: Thermal
### Reduce Water Temperature via Ventilation
`sys-req~thermal1~1`

The control system shall reduce the water temperature by actuating ventilation fans depending on the actual water temperature.
Needs: swreq

### Read Ventilation Target Temperature Window
`sys-req~thermal2~1`

The control system shall repeatedly read the target temperature window for the ventilation control from the database.
Needs: swreq

### Stop Ventilation Control Remotely
`sys-req~thermal3~1`

The control system shall allow the user to stop the ventilation control.
Needs: swreq

### Restart Ventilation Control Remotely
`sys-req~thermal4~1`

The control system shall allow the user to re-start the ventilation control.
Needs: swreq

### Increase Water Temperature via Heater
`sys-req~thermal5~1`

The control system shall increase the water temperature by actuating a heater depending on the actual water temperature.
Needs: swreq

### Read Heating Target Temperature Window
`sys-req~thermal6~1`

The control system shall repeatedly read the target temperature window for the heating control from the database.
Needs: swreq

### Stop Heating Control Remotely
`sys-req~thermal7~1`

The control system shall allow the user to stop the heating control.
Needs: swreq

### Restart Heating Control Remotely
`sys-req~thermal8~1`

The control system shall allow the user to re-start the heating control.
Needs: swreq

### Inhibit Heater on Low Sump Water Level
`sysreq~thermal9~1`

The control system shall inhibit the heater if the tank level switch reports a low water level for an extended time period.
Needs: swreq

## sys-watchmen: Watchmen
### Self Monitoring and Liveness
`sys-req~watchmen~1`

The control system shall monitor itself.
Needs: swreq

### Pet Hardware Watchdog
`sysreq~watchmen2~1`

The control system shall pet the hardware watchdog at regular intervals to verify process liveness.
Needs: swreq

### Detect Execution Stalls and Trigger Reboot
`sysreq~watchmen3~1`

The control system shall detect execution stalls or component failures and signal the watchdog to trigger a safe system reboot.
Needs: swreq

## sys-recorder: Recorder
### Record Measurement Signals in Database
`sys-req~recorder~1`

The control system shall regularly record measurement signals in the database.
Needs: swreq

### Filter Raw Sensor Readings
`sysreq~recorder2~1`

The control system shall filter raw sensor readings using an infinite impulse response (IIR) filter to eliminate measurement noise.
Needs: swreq

## sys-utilities: Utilities
### Process OS Shutdown Signals
`sys-req~utilities1~1`

The control system shall receive signals from the operating system and react accordingly.
Needs: swreq

### Read Configuration File on Startup
`sysreq~utilities2~1`

The control system shall read all configuration parameters from a local configuration file on startup.
Needs: swreq

### Verify Version Hash Compatibility
`sysreq~utilities3~1`

The control system shall verify that its internal version information and hash are compatible with the database schema on startup.
Needs: swreq

### Publish Process ID File
`sysreq~utilities4~1`

The control system shall publish its process ID (PID) to a file on startup to prevent duplicate running instances.
Needs: swreq

## sys-dispatch: Dispatch
### Receive POSIX Queue Messages
`sysreq~messaging~1`

The control system shall receive POSIX messages from the operating system and react accordingly.
Needs: swreq
