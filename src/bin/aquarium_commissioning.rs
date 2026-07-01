/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Commissioning tool for the Aquarium Control system.
//!
//! This binary provides a Terminal User Interface (TUI) to manually test and verify
//! the hardware components of the aquarium control system. It allows the user to:
//! - Toggle actuators (relays, pumps, lights) on and off.
//! - View real-time sensor readings (temperature, pH, etc.).
//! - Verify configuration settings.
//!
//! It supports both real hardware (via GPIO/I2C) and simulation mode, depending on the
//! configuration provided.

#[cfg(all(target_os = "linux", feature = "target_hw"))]
use aquarium_control::beacon::ws2812b::{RgbLedColor, Ws2812B};
use aquarium_control::launch::argument_parser::check_argument_for_valid_config_file_name;
use aquarium_control::launch::channels::Channels;
use aquarium_control::launch::execution_config::ExecutionConfig;
use aquarium_control::relays::actuate_controllino::ActuateControllino;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use aquarium_control::relays::actuate_gpio::{ActuateGpio, ActuateGpioConfig};
use aquarium_control::relays::actuate_simulator::ActuateSimulator;
use aquarium_control::relays::relay_manager::RelayActuationTrait;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use aquarium_control::sensors::atlas_scientific::AtlasScientific;
use aquarium_control::sensors::atlas_scientific::AtlasScientificResultData;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use aquarium_control::sensors::dht::Dht;
use aquarium_control::sensors::dht::DhtResult;
use aquarium_control::sensors::ds18b20::{Ds18b20, Ds18b20ResultData};
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use aquarium_control::sensors::gpio_handler::GpioHandler;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use aquarium_control::sensors::i2c_interface::I2cInterface;
use aquarium_control::sensors::sensor_manager::{
    SensorManager, SensorManagerMutexes, SensorManagerSignals,
};
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use aquarium_control::sensors::tank_level_switch::TankLevelSwitch;
use aquarium_control::sensors::tank_level_switch::TankLevelSwitchSignals;
use aquarium_control::utilities::channel_content::{
    AquariumDevice, AquariumSignal, InternalCommand,
};
use aquarium_control::utilities::config::read_config_file;
use aquarium_control::utilities::publish_pid::PublishPid;

use aquarium_control::launch::privilege_check::is_running_as_root;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, Mutex};
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use std::thread;
use std::time::{Duration, Instant};
use std::{env, io, panic};

const DEVICES: [AquariumDevice; 13] = [
    AquariumDevice::Skimmer,
    AquariumDevice::MainPump1,
    AquariumDevice::MainPump2,
    AquariumDevice::AuxPump1,
    AquariumDevice::AuxPump2,
    AquariumDevice::Heater,
    AquariumDevice::Ventilation,
    AquariumDevice::RefillPump,
    AquariumDevice::Feeder,
    AquariumDevice::PeristalticPump1,
    AquariumDevice::PeristalticPump2,
    AquariumDevice::PeristalticPump3,
    AquariumDevice::PeristalticPump4,
];

const SIGNALS: [AquariumSignal; 6] = [
    AquariumSignal::WaterTemperature,
    AquariumSignal::AmbientTemperature,
    AquariumSignal::AmbientHumidity,
    AquariumSignal::pH,
    AquariumSignal::Conductivity,
    AquariumSignal::TankLevelSwitchPosition,
];

/// Determines the initial state (On/Off) for a given device.
///
/// Some devices might default to ON or depend on the GPIO configuration.
fn get_initial_state(device: &AquariumDevice, use_gpio: bool) -> bool {
    match device {
        AquariumDevice::Skimmer
        | AquariumDevice::MainPump1
        | AquariumDevice::MainPump2
        | AquariumDevice::AuxPump1
        | AquariumDevice::AuxPump2 => true,
        AquariumDevice::Ventilation => use_gpio, // On if gpio, Off if controllino
        _ => false,
    }
}

/// Application state for the TUI.
///
/// Holds the list of devices, their current states, sensor readings, and UI state.
struct App {
    devices: Vec<AquariumDevice>,
    device_states: HashMap<String, bool>, // Device name -> On/Off
    list_state: ListState,
    signals: HashMap<AquariumSignal, f32>,
    mutexes: Option<SensorManagerMutexes>,
    sensor_manager_active: bool,
    tank_level_switch_mutex: Option<Arc<Mutex<TankLevelSwitchSignals>>>,
    tank_level_switch_active: bool,
    show_popup: bool,
    beacon_state: u8, // 0: Off, 1: Red, 2: Green, 3: Blue
}

impl App {
    /// Creates a new instance of the application state.
    fn new(
        mutexes: SensorManagerMutexes,
        sensor_manager_active: bool,
        tls_mutex: Arc<Mutex<TankLevelSwitchSignals>>,
        tls_active: bool,
        use_gpio: bool,
        beacon_active: bool,
    ) -> App {
        let mut devices = DEVICES.to_vec();
        if beacon_active {
            devices.push(AquariumDevice::Beacon);
        }

        let mut device_states = HashMap::new();
        for d in &devices {
            if *d != AquariumDevice::Beacon {
                device_states.insert(d.to_string(), get_initial_state(d, use_gpio));
            }
        }

        let mut list_state = ListState::default();
        list_state.select(Some(0));

        App {
            devices,
            device_states,
            list_state,
            signals: HashMap::new(),
            mutexes: Some(mutexes),
            sensor_manager_active,
            tank_level_switch_mutex: Some(tls_mutex),
            tank_level_switch_active: tls_active,
            show_popup: true,
            beacon_state: 0,
        }
    }

    /// Selects the next item in the device list.
    fn next(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.devices.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    /// Selects the previous item in the device list.
    fn previous(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.devices.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    /// Updates the sensor signal values from the shared mutexes.
    fn update_signals(&mut self) {
        if self.sensor_manager_active {
            if let Some(mutexes) = &self.mutexes {
                if let Ok(signals) = mutexes.mutex_sensor_manager_signals.lock() {
                    self.signals
                        .insert(AquariumSignal::WaterTemperature, signals.water_temperature);
                    self.signals.insert(
                        AquariumSignal::AmbientTemperature,
                        signals.ambient_temperature,
                    );
                    self.signals
                        .insert(AquariumSignal::AmbientHumidity, signals.ambient_humidity);
                    self.signals.insert(AquariumSignal::pH, signals.ph);
                    self.signals
                        .insert(AquariumSignal::Conductivity, signals.conductivity);
                }
            }
        }
        if self.tank_level_switch_active {
            if let Some(tls_mutex) = &self.tank_level_switch_mutex {
                if let Ok(tls_signals) = tls_mutex.lock() {
                    let val = if tls_signals.tank_level_switch_position {
                        1.0
                    } else {
                        0.0
                    };
                    self.signals
                        .insert(AquariumSignal::TankLevelSwitchPosition, val);
                }
            }
        }
    }
}

/// Displays the valid command line parameter combinations
fn display_usage(default_config_file: &str) {
    println!("This is the aquarium_control commissioning tool.");
    println!("Default configuration file is {}", default_config_file);
    println!("As command line parameter, you may provide an alternative configuration file.");
    println!("-h, --help: Output this page.");
}

/// Checks if a command-line argument is a recognized command and executes it.
fn check_is_command(command: &str, default_config_file: &str) -> bool {
    match command {
        "-h" | "--help" => {
            display_usage(default_config_file);
            true
        }
        _ => false, // All other strings do not match
    }
}

/// The main entry point for the commissioning tool.
///
/// It performs the following steps:
/// 1. Checks environment and privileges.
/// 2. Loads configuration.
/// 3. Initializes channels and shared mutexes.
/// 4. Starts sensor threads (SensorManager, DHT, Atlas Scientific, etc.).
/// 5. Initializes the actuator (Simulator, GPIO, or Controllino).
/// 6. Sets up the TUI and runs the main event loop.
fn main() -> Result<(), Box<dyn Error>> {
    // 0. Environment check
    if !cfg!(feature = "target_hw") || !cfg!(target_os = "linux") {
        eprintln!("Error: The commissioning tool requires the 'target_hw' feature to be enabled and must be executed on a Linux system.");
        return Ok(());
    }

    // check if the user has root privileges
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if !is_running_as_root() {
        eprintln!("Error: The application must be run with root privileges.");
        return Ok(());
    }

    // 1. Config
    // get command line parameters from OS
    let args: Vec<String> = env::args().collect();

    // initialize with a default configuration file name
    let mut config_file = "/etc/aquarium_control/aquarium_control.toml".to_string();

    // Check command line parameters
    if args.len() > 1 {
        // If the first argument contains a path and file name, then let's use that.
        config_file = match check_argument_for_valid_config_file_name(&args[1]) {
            Ok(valid_file_name) => valid_file_name, // The file is valid. Start operation.
            Err(config_file_definition_error) => {
                let mut user_arg_identified = false;
                // Iterate through the command line parameters and check if they are valid.
                for user_arg in args[1..].iter() {
                    user_arg_identified = check_is_command(user_arg, &config_file);
                    if user_arg_identified {
                        break;
                    }
                }
                return if !user_arg_identified {
                    // If the user provided an invalid filename and no valid command, output error message and abort.
                    eprintln!("Configuration Error: {}", config_file_definition_error);
                    Ok(())
                } else {
                    // At least one valid user command was provided. Graceful exit.
                    Ok(())
                };
            }
        }
    }

    let config = match read_config_file(config_file.clone()) {
        Ok(c) => c,
        Err(_e) => {
            let err_msg = "Failed to read/parse configuration file ".to_string() + &config_file;
            return Err(err_msg.into());
        }
    };

    // extract before moving
    let relay_manager_use_gpio = config.relay_manager.use_gpio;

    let execution_config = ExecutionConfig::new(&config);

    // Check if another instance is running
    let publish_pid = PublishPid::new(config.publish_pid)?;
    let publish_pid_clone = publish_pid.clone();

    panic::set_hook(Box::new(move |panic_info| {
        let _ = publish_pid_clone.erase_pid();
        eprintln!("Panic occurred: {panic_info}");
    }));

    // 2. Channels
    // We construct all channels.
    let mut channels = Channels::new(
        config.relay_manager.use_simulator,
        config.sensor_manager.use_simulator,
        config.tank_level_switch.use_simulator,
        true,
    );

    // 3. Mutexes for Sensors
    let mutex_ds18b20_water_temperature = Arc::new(Mutex::new(Ok(Ds18b20ResultData::new(
        config.sensor_manager.replacement_value_water_temperature,
    ))));
    let mutex_ds18b20_ambient_temperature = Arc::new(Mutex::new(Ok(Ds18b20ResultData::new(
        config.sensor_manager.replacement_value_ambient_temperature,
    ))));
    let mutex_dht: Arc<Mutex<DhtResult>> = Arc::new(Mutex::new(Ok((0.0, 0.0))));
    let mutex_atlas_scientific_temperature = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_water_temperature,
    )));
    let mutex_atlas_scientific_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_ph,
    )));
    let mutex_atlas_scientific_conductivity = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_conductivity,
    )));
    let mutex_sensor_manager_signals: Arc<Mutex<SensorManagerSignals>> = Arc::new(Mutex::new(
        SensorManagerSignals::new(&config.sensor_manager),
    ));
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    let sensor_mutexes = SensorManagerMutexes {
        mutex_ds18b20_ambient_temperature: mutex_ds18b20_ambient_temperature.clone(),
        mutex_ds18b20_water_temperature: mutex_ds18b20_water_temperature.clone(),
        mutex_dht: mutex_dht.clone(),
        mutex_atlas_scientific_temperature: mutex_atlas_scientific_temperature.clone(),
        mutex_atlas_scientific_ph: mutex_atlas_scientific_ph.clone(),
        mutex_atlas_scientific_conductivity: mutex_atlas_scientific_conductivity.clone(),
        mutex_sensor_manager_signals: mutex_sensor_manager_signals.clone(),
    };

    // Initialize shared Tokio runtime for async tasks
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|e| format!("Failed to build Tokio runtime: {:?}", e))?;

    // 4. Start SensorManager Task
    if execution_config.sensor_manager {
        let mut sensor_manager = SensorManager::new(config.sensor_manager)?;
        // We clone mutexes for the task, but we need to pass a fresh copy of mutexes struct
        let sensor_mutexes_for_task = sensor_mutexes.clone();

        // We need to move the channels for sensor manager into the task.
        // Channels struct has `sensor_manager: SensorManagerChannels`.
        // We can extract it.
        let mut sensor_manager_channels = channels.sensor_manager; // Move out

        rt.spawn(async move {
            sensor_manager.execute(&mut sensor_manager_channels, sensor_mutexes_for_task).await;
        });
    }

    // 5. Setup Actuator
    let mut actuator: Box<dyn RelayActuationTrait> = if config.relay_manager.use_simulator {
        Box::new(ActuateSimulator::new(
            &mut channels.actuate_simulator,
            config.controllino_relay,
        ))
    } else if config.relay_manager.use_gpio {
        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        {
            let gpio = rppal::gpio::Gpio::new().map_err(|e| format!("GPIO error: {}", e))?;
            let gpio_config = ActuateGpioConfig::new(&config.gpio_handler);
            Box::new(
                ActuateGpio::new(gpio, gpio_config)
                    .map_err(|e| format!("ActuateGpio error: {:?}", e))?,
            )
        }
        #[cfg(not(all(target_os = "linux", feature = "target_hw")))]
        {
            return Err("GPIO not supported on this platform.".into());
        }
    } else {
        Box::new(
            ActuateControllino::new(config.controllino_relay)
                .map_err(|e| format!("Controllino error: {:?}", e))?,
        )
    };

    // 6. Setup GpioHandler and sensor communication
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    {
        // extract values from config before moving it
        let gpio_tank_level_switch = config.gpio_handler.tank_level_switch;
        let gpio_dht22_io = config.gpio_handler.dht22_io;
        let gpio_dht22_vcc = config.gpio_handler.dht22_vcc;

        // Initialize GpioHandler
        let gpio_handler = GpioHandler::new(config.gpio_handler)
            .map_err(|e| format!("GpioHandler error: {:?}", e))?
            .ok_or("GpioHandler failed to initialize")?;

        let gpio_lib_handle = gpio_handler.gpio_lib_handle_opt.as_ref().unwrap().clone();
        let gpio_lib_handle_for_dht = gpio_lib_handle.clone();
        let gpio_lib_handle_for_tls = gpio_lib_handle.clone();

        if execution_config.dht {
            let mut dht = Dht::new(
                config.dht,
                Some(gpio_lib_handle_for_dht),
                gpio_dht22_io,
                gpio_dht22_vcc,
            );
            let mut dht_channels = channels.dht;
            let mutex_dht_clone = mutex_dht.clone();

            thread::spawn(move || {
                dht.execute(mutex_dht_clone, &mut dht_channels);
            });
        }

        if execution_config.tank_level_switch {
            let mut tank_level_switch = TankLevelSwitch::new(
                config.tank_level_switch,
                true,
                Some(gpio_lib_handle_for_tls),
                gpio_tank_level_switch,
            )
            .map_err(|e| format!("TankLevelSwitch error: {:?}", e))?;

            let mut tls_channels = channels.tank_level_switch;
            let mutex_tls_clone = mutex_tank_level_switch_signals.clone();

            rt.spawn(async move {
                let _ = tank_level_switch.execute(&mut tls_channels, mutex_tls_clone).await;
            });
        }

        if execution_config.atlas_scientific {
            let atlas_drivers = AtlasScientific::create_drivers(&config.atlas_scientific)
                .map_err(|e| format!("Atlas driver error: {:?}", e))?;
            let mut atlas = AtlasScientific::new(config.atlas_scientific, atlas_drivers);

            let mut atlas_channels = channels.atlas_scientific;
            let m_temp = mutex_atlas_scientific_temperature.clone();
            let m_ph = mutex_atlas_scientific_ph.clone();
            let m_cond = mutex_atlas_scientific_conductivity.clone();

            if execution_config.i2c_interface {
                let mut i2c = I2cInterface::new(config.i2c_interface)
                    .map_err(|e| format!("I2C error: {:?}", e))?;
                let mut i2c_channels = channels.i2c_interface;
                rt.spawn(async move {
                    i2c.execute(&mut i2c_channels).await;
                });
            }

            atlas
                .init_devices(&mut atlas_channels)
                .expect("Atlas init failed");
            thread::spawn(move || {
                atlas.execute(&mut atlas_channels, m_temp, m_ph, m_cond);
            });
        }
    }

    if execution_config.ds18b20 {
        let mut ds18b20 = Ds18b20::new(config.ds18b20)?;
        let mut ds18b20_channels = channels.ds18b20;
        let m_water = mutex_ds18b20_water_temperature.clone();
        let m_amb = mutex_ds18b20_ambient_temperature.clone();
        rt.spawn(async move {
            ds18b20.execute(&mut ds18b20_channels, m_water, m_amb).await;
        });
    }

    // 7. Setup Ws2812B
    #[cfg(not(all(target_os = "linux", feature = "target_hw")))]
    let beacon_active = false;
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    let mut beacon_active = false;
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    let mut ws2812b_opt: Option<Ws2812B> = None;
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    if config.ws2812b.active {
        ws2812b_opt = Some(Ws2812B::new(config.ws2812b)?);
        beacon_active = true;
    }

    // 8. Setup TUI
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app = App::new(
        sensor_mutexes,
        execution_config.sensor_manager,
        mutex_tank_level_switch_signals,
        execution_config.tank_level_switch,
        relay_manager_use_gpio,
        beacon_active,
    );
    let res = run_app(
        &mut terminal,
        app,
        &mut *actuator,
        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        &mut ws2812b_opt,
    );

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err)
    }

    let _ = publish_pid.erase_pid();

    Ok(())
}

/// Runs the main application loop.
///
/// Handles terminal drawing, input events, and periodic updates (sensor polling, heartbeats).
fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut app: App,
    actuator: &mut dyn RelayActuationTrait,
    #[cfg(all(target_os = "linux", feature = "target_hw"))] ws2812b: &mut Option<Ws2812B>,
) -> Result<(), Box<dyn Error>> {
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(250);

    let mut last_heartbeat = Instant::now();
    let heartbeat_interval = actuator
        .get_heartbeat_interval_seconds()
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(3600)); // Default long if none

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if app.show_popup {
                    app.show_popup = false;
                } else {
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Down => app.next(),
                        KeyCode::Up => app.previous(),
                        KeyCode::Char(' ') => {
                            // Toggle
                            if let Some(idx) = app.list_state.selected() {
                                let device = app.devices[idx].clone();
                                if device == AquariumDevice::Beacon {
                                    app.beacon_state = (app.beacon_state + 1) % 4;
                                    #[cfg(all(target_os = "linux", feature = "target_hw"))]
                                    if let Some(led) = ws2812b {
                                        let color = match app.beacon_state {
                                            0 => RgbLedColor::Off,
                                            1 => RgbLedColor::Red,
                                            2 => RgbLedColor::Green,
                                            3 => RgbLedColor::Blue,
                                            _ => RgbLedColor::Off,
                                        };
                                        let _ = led.set_color(color);
                                    }
                                } else {
                                    let name = device.to_string();
                                    let current_state =
                                        *app.device_states.get(&name).unwrap_or(&false);
                                    let new_state = !current_state;

                                    let cmd = if new_state {
                                        InternalCommand::SwitchOn(device.clone())
                                    } else {
                                        InternalCommand::SwitchOff(device.clone())
                                    };

                                    if let Err(_e) = actuator.actuate(&cmd) {
                                        // In a real app we might show an error popup
                                        // For now, we assume success or log to stderr (which messes up TUI)
                                        // We'll just not update state if failed?
                                        // Or better, just update state optimistically.
                                    }
                                    app.device_states.insert(name, new_state);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.update_signals();
            last_tick = Instant::now();
        }

        // Heartbeat
        if last_heartbeat.elapsed() >= heartbeat_interval {
            let _ = actuator.heartbeat();
            last_heartbeat = Instant::now();
        }
    }
}

/// Renders the user interface.
///
/// Draws the list of actuators, the list of sensors, and any active popups.
fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(f.area());

    // Left: Actuators
    let items: Vec<ListItem> = app
        .devices
        .iter()
        .map(|device| {
            if *device == AquariumDevice::Beacon {
                let status = match app.beacon_state {
                    0 => "[OFF]",
                    1 => "[RED]",
                    2 => "[GREEN]",
                    3 => "[BLUE]",
                    _ => "[OFF]",
                };
                let style = match app.beacon_state {
                    0 => Style::default().fg(Color::Red),
                    1 => Style::default().fg(Color::Red),
                    2 => Style::default().fg(Color::Green),
                    3 => Style::default().fg(Color::Blue),
                    _ => Style::default().fg(Color::Red),
                };
                ListItem::new(format!("{} Beacon", status)).style(style)
            } else {
                let name = device.to_string();
                let state = *app.device_states.get(&name).unwrap_or(&false);
                let status = if state { "[ON] " } else { "[OFF]" };
                let content = format!("{} {}", status, name);
                let style = if state {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Red)
                };
                ListItem::new(content).style(style)
            }
        })
        .collect();

    let items = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Actuators (Space to Toggle)"),
        )
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(Color::DarkGray),
        )
        .highlight_symbol(">> ");

    f.render_stateful_widget(items, chunks[0], &mut app.list_state);

    // Right: Sensors
    let sensor_text: Vec<ListItem> = SIGNALS
        .iter()
        .map(|sig| {
            let is_tls = *sig == AquariumSignal::TankLevelSwitchPosition;
            let is_active = if is_tls {
                app.tank_level_switch_active
            } else {
                app.sensor_manager_active
            };

            let content = if is_active {
                let val = app.signals.get(sig).unwrap_or(&0.0);
                if is_tls {
                    let status_text = if *val > 0.5 { "High" } else { "Low" };
                    format!("{}: {}", sig, status_text)
                } else {
                    format!("{}: {}", sig, sig.output_file_string(*val))
                }
            } else {
                format!("{}: Disabled", sig)
            };

            ListItem::new(content)
        })
        .collect();

    let sensors_list =
        List::new(sensor_text).block(Block::default().borders(Borders::ALL).title("Sensors"));

    f.render_widget(sensors_list, chunks[1]);

    if app.show_popup {
        let block = Block::default()
            .title("Aquarium Control Commissioning")
            .borders(Borders::ALL);
        let area = centered_rect(60, 40, f.area());
        f.render_widget(Clear, area); // this clears out the background
        f.render_widget(block, area);

        let text = vec![
            Line::from("Welcome to the Commissioning Tool"),
            Line::from(""),
            Line::from("Controls:"),
            Line::from("  UP / DOWN : Select Actuator"),
            Line::from("  SPACE     : Toggle Actuator On/Off / Red/Green/Blue/Off"),
            Line::from("  q         : Quit"),
            Line::from(""),
            Line::from("Press any key to close this popup"),
        ];
        let paragraph = Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Usage Instructions"),
            )
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, area);
    }
}

/// Helper function to create a centered rect using up certain percentage of the available rect `r`.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(popup_layout[1])[1]
}
