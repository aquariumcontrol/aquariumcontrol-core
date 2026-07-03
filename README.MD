The main control application of Aquarium control implements the following features:

* [Data acquisition of water temperature, pH and conductivity](Data acquisition of water temperature, pH and conductivity)
* [Data acquisition of ambient temperature and humidity](Data acquisition of ambient temperature and humidity)
* [Refill control for fresh water](Fresh water refill control)
* Feed control
* Water temperature control using air ventilation and heater
* Balling mineral dosing

The control of the relays for operating the devices can use two different modes:
* [Using an Arduino-based relay actuation](Arduino-based relay actuation)
* Using the GPIO of the Raspberry Pi (requires additional hardware)

Additionally, the main control application implements the following supporting features:
* [Version comparison between database and application](Version comparison between database and application)
* Configuration using a Rust .toml configuration file
* Logging
* Command interface to receive instructions from outside of the application via a POSIX message queue
* Storage of recorded data in SQL database
* File-based communication to RAM-disk
* Usage of simulator to run with simulated sensor data for development purposes
* Schedule checks to limit the hour of day of operating certain actuators
* [Communication with HW Watchdog](Communication with HW Watchdog)
* [Temperature gradient calculation](Temperature gradient calculation)

The application is written in Rust and is [automatically started using systemd](automatic start by using systemd).

The documentation of the software is available [here](http://vps2483992.servdiscount-customer.com/aquarium_control_doc/aquarium_control/).

Development for the main control application requires the [setup of the development environment](setup of the development environment).

There is a [release procedure](Release procedure) which is highly recommended to be followed also by anyone developing the SW further.

# Architecture
## Startup of the application
The startup uses a two-layer approach:
1. `main()` - Simple error handler wrapper that calls `run()` and exits with code 1 on errors
2. `run()` - The actual initialization orchestrator that performs the major steps

### Phase 1: Command Line & Configuration
* Parses command line arguments ("help", "version", config file)
* Sets the default config file: `/etc/aquarium_control/aquarium_control.toml`
* Loads configuration and creates `ExecutionConfig` (boolean flags determining which modules to run)

### Phase 2: System Setup
* Initializes the logging system
* Logs version information and executable hash
* Verifies root privileges (required for hardware access)
* Publishes process ID to file for client communication

### Phase 3: Database & Hardware
* Connects to database (MariaDB)
* Creates component-specific SQL interfaces (`Schedule`, `Balling`, `Refill`, `Heating`, `Feed`, `Data`)
* Validates database schema and version compatibility
* Initializes hardware components if configured:
  * RGB LEDs
  * GPIO handlers
  * Tank level switch
  * I2C interface
  * Atlas Scientific sensors
  * DHT temperature/humidity sensors
  * Relay actuator (Controllino)

### Phase 4: Communication Channels
* Creates the `Channels` struct containing ~30+ inter-thread communication channels
* Uses custom `AquaSender`/`AquaReceiver` wrappers for MPSC channels
* Establishes bidirectional and unidirectional message paths between all modules

### Phase 5: Component Initialization
Instantiates all high-level control components:
* data logger
* sensor manager
* relay manager
* food injection
* mineral dosing (balling)
* water injection/refill
* heating control
* ventilation control
* monitors
* watchdog
* schedule checker
* temperature gradient calculation
* IPC messaging system

### Phase 6: Thread Spawning
* Uses `std::thread::scope` for guaranteed cleanup
* Spawns threads conditionally based on `ExecutionConfig` flags
* Critical: A 3-second startup delay allows sensors to acquire first data before control loops activate

### Architecture of thread configuration
* The `Channels` module (`src/launch/channels.rs`) acts as a central wiring diagram, with the Signal Handler serving as the main event coordinator.
* The `ExecutionConfig` decouples configuration from execution - threads only receive flags about which modules are active, not the full config - this also avoids ownership issues.

## Threads
The application spawns the following threads:

| Thread name | Purpose |
|---|---|
| signal_handler | Receiving signals from the operating system |
| schedule_check | Polling the database to capture changes of the actuator channel. Informing other threads about it. |
| sensor_manager | Data acquisition |
| relay_manager | Actuation of relays |
| data_logger | Recording of data |
| tank_level_switch | Postprocessing of tank level switch position signal |
| refill | Fresh water refill control |
| heating | Heating control |
| ventilation | Ventilation control |
| balling | Balling mineral dosing control |
| watchdog | Sending alive signal to Raspberry Pi hardware watchdog |
| monitors | Diagnostics (not implemented as of January, 2026) |
| messaging | Receiving communication via message queue |
| memory | Monitoring memory utilization |
| ds18b20 | Recording data from DS18B20 temperature sensor |
| feed | Feed control |
| i2c_interface | Communication via I2C |
| atlas_scientific | Recording data from Atlas Scientific temperature sensor |
| dht | Recording data from DHT sensor |
| publish_pid | Writing PID to lock file |
| temperature_gradient | Temperature gradient calculation |

# Measurement using Atlas Scientific circuits
The core Abstraction (`AtlasScientificDriver` Trait) is defined in `src/sensors/atlas_scientific_driver.rs`, this trait serves as the common interface for all sensor types. It enforces three key behaviors:
* `send_request_to_i2c_interface`: Encapsulates how to trigger a measurement (e.g., constructing specific I2C commands).
* `receive_response_from_i2c_interface`: Handles parsing the raw byte response from the I2C bus into a normalized floating-point value.
* `device_init`: Performs hardware-specific setup (crucial for OEM type sensor circuit).

Circuit-type-specific implementations
* OEM Driver (`AtlasScientificOem`):
  * Protocol: Uses a binary, register-based protocol.
  * Initialization: Requires an explicit "Wake Up" command (writing to `REG_WAKEUP`) and verifies the hardware by reading the `REG_DEVICE_TYPE`.
  * Read Cycle: Writes a register address (e.g., `REG_DATA_MSB_PH`), waits for a short processing time, and reads 4 bytes.
  * Parsing: Decodes the 4-byte response as a Big Endian unsigned integer and divides it by a specific scale factor (e.g., 1000.0) to get the float value.
* EZO Driver (`AtlasScientificEzo`):
  * Protocol: Uses a text-based (ASCII) command protocol.
  * Initialization: Minimal or no-op (these devices are typically always ready or auto-on).
  * Read Cycle: Sends the ASCII character 'R', waits for a longer processing time, and reads a fixed 8-byte response.
  * Parsing: Validates "magic" start/end bytes (1 and 0), converts the inner ASCII string (e.g., "23.50") to a float.

Context and factory (`AtlasScientific` struct)
* The main `AtlasScientific` struct acts as the context. It holds a `HashMap<AquariumSignal`, `Box<dyn AtlasScientificDriver>>`.
* Factory Logic: The `create_drivers` method functions as a factory. It reads the `aquarium_control.toml` configuration (specifically fields like `sensor_type_ph = "oem"` or `"ezo"`) and
instantiates the correct driver implementation for each sensor (temperature, pH, conductivity).
* Execution: The main thread loop calls `driver.send_request...` and `driver.receive_response...` on the active driver, unaware of whether it is communicating via binary registers or ASCII commands.

Communication Flow

The architecture decouples protocol logic from bus I/O:
1. Driver creates an abstraction-specific `I2cRequest` (containing address, write buffer, read length, delay).
2. `AtlasScientific` thread sends this request via a channel to the `I2cInterface` thread.
3. `I2cInterface` thread performs the physical I2C transaction.
4. `AtlasScientific` thread receives the raw bytes and passes them back to the driver for parsing.
5. Additionally, `AtlasScientific` performs measurement value checks informing deviations to `Monitors`.

This design enables the system to mix and match sensor types (e.g., an OEM type pH sensor circuit with an EZO type temperature sensor circuit) purely through configuration changes.

# Monitors
The Monitors module (`src/watchmen/monitors.rs`) is a dedicated thread that aggregates and stores historical system state information.
It interacts with the following modules:
* Refill
* Signal handler

The Monitors thread receives data from the other thread by polling the channels.

The data is transferred in specific structs ("Views"), e.g. `RefillView`.

`Monitors` contains a deduplication logic: Data is only stored when it differs from previous sample.

The number of samples is also limited (rolling window), to avoid overflow.

# Commissioning
For the commissioning of the aquarium control, there is a separate binary which resides in `src/bin/commissioning.rs`.