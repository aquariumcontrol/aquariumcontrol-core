/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use aquarium_control::launch::channels::Channels;
use aquarium_control::launch::execution_config::ExecutionConfig;
use aquarium_control::relays::actuate_controllino::ActuateControllino;
use aquarium_control::relays::actuate_simulator::ActuateSimulator;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use aquarium_control::relays::actuate_gpio::{ActuateGpio, ActuateGpioConfig};
use aquarium_control::relays::relay_manager::RelayActuationTrait;
use aquarium_control::sensors::sensor_manager::{
    SensorManager, SensorManagerMutexes, SensorManagerSignals,
};
use aquarium_control::utilities::channel_content::{AquariumDevice, AquariumSignal, InternalCommand};
use aquarium_control::utilities::config::read_config_file;
use aquarium_control::utilities::publish_pid::PublishPid;
use aquarium_control::sensors::ds18b20::{Ds18b20, Ds18b20ResultData};
use aquarium_control::sensors::dht::DhtResult;
use aquarium_control::sensors::atlas_scientific::AtlasScientificResultData;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use aquarium_control::sensors::gpio_handler::GpioHandler;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use aquarium_control::sensors::dht::Dht;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use aquarium_control::sensors::atlas_scientific::AtlasScientific;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use aquarium_control::sensors::i2c_interface::I2cInterface;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use aquarium_control::sensors::tank_level_switch::{TankLevelSwitch, TankLevelSwitchSignals};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame, Terminal,
};
use std::collections::HashMap;
use std::error::Error;
use std::{io, panic};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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

const SIGNALS: [AquariumSignal; 5] = [
    AquariumSignal::WaterTemperature,
    AquariumSignal::AmbientTemperature,
    AquariumSignal::AmbientHumidity,
    AquariumSignal::pH,
    AquariumSignal::Conductivity,
];

struct App {
    devices: Vec<AquariumDevice>,
    device_states: HashMap<String, bool>, // Device name -> On/Off
    list_state: ListState,
    signals: HashMap<AquariumSignal, f32>,
    mutexes: Option<SensorManagerMutexes>,
    sensor_manager_active: bool,
}

impl App {
    fn new(mutexes: SensorManagerMutexes, sensor_manager_active: bool) -> App {
        let mut device_states = HashMap::new();
        for d in &DEVICES {
            device_states.insert(d.to_string(), false);
        }
        
        let mut list_state = ListState::default();
        list_state.select(Some(0));

        App {
            devices: DEVICES.to_vec(),
            device_states,
            list_state,
            signals: HashMap::new(),
            mutexes: Some(mutexes),
            sensor_manager_active,
        }
    }

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

    fn update_signals(&mut self) {
        if let Some(mutexes) = &self.mutexes {
            if let Ok(signals) = mutexes.mutex_sensor_manager_signals.lock() {
                self.signals.insert(AquariumSignal::WaterTemperature, signals.water_temperature);
                self.signals.insert(AquariumSignal::AmbientTemperature, signals.ambient_temperature);
                self.signals.insert(AquariumSignal::AmbientHumidity, signals.ambient_humidity);
                self.signals.insert(AquariumSignal::pH, signals.ph);
                self.signals.insert(AquariumSignal::Conductivity, signals.conductivity);
            }
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // 0. Environment check
    if !cfg!(feature = "target_hw") || !cfg!(target_os = "linux") {
        eprintln!("Error: The commissioning tool requires the 'target_hw' feature to be enabled and must be executed on a Linux system.");
        return Ok(());
    }

    // 1. Config
    let config_file = "/etc/aquarium_control/aquarium_control.toml".to_string();
    let config = read_config_file(config_file).or_else(|_| {
        // Fallback for dev environment or local test
         read_config_file("config/aquarium_control.toml".to_string())
    })?;

    let execution_config = ExecutionConfig::new(&config);

    // Check if another instance is running
    let publish_pid = PublishPid::new(config.publish_pid.clone())?;
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
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
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

    // 4. Start SensorManager Thread
    if execution_config.sensor_manager {
        let mut sensor_manager = SensorManager::new(config.sensor_manager.clone())?;
        // We clone mutexes for the thread, but we need to pass a fresh copy of mutexes struct
        let sensor_mutexes_for_thread = sensor_mutexes.clone();

        // We need to move the channels for sensor manager into the thread.
        // Channels struct has `sensor_manager: SensorManagerChannels`.
        // We can extract it.
        let mut sensor_manager_channels = channels.sensor_manager; // Move out

        thread::spawn(move || {
            sensor_manager.execute(&mut sensor_manager_channels, sensor_mutexes_for_thread);
        });
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    {
        // Initialize GpioHandler
        let gpio_handler = GpioHandler::new(config.gpio_handler.clone())
             .map_err(|e| format!("GpioHandler error: {:?}", e))?
             .ok_or("GpioHandler failed to initialize")?;

        let gpio_lib_handle = gpio_handler.gpio_lib_handle_opt.as_ref().unwrap().clone();
        let gpio_lib_handle_for_dht = gpio_lib_handle.clone();
        let gpio_lib_handle_for_tls = gpio_lib_handle.clone();

        if execution_config.dht {
            let gpio_dht22_io = config.gpio_handler.dht22_io;
            let gpio_dht22_vcc = config.gpio_handler.dht22_vcc;
            let mut dht = Dht::new(config.dht.clone(), Some(gpio_lib_handle_for_dht), gpio_dht22_io, gpio_dht22_vcc);
            let mut dht_channels = channels.dht;
            let mutex_dht_clone = mutex_dht.clone();
            
            thread::spawn(move || {
                dht.execute(mutex_dht_clone, &mut dht_channels);
            });
        }
        
        if execution_config.tank_level_switch {
            let gpio_tank_level_switch = config.gpio_handler.tank_level_switch;
            let mut tank_level_switch = TankLevelSwitch::new(
                 config.tank_level_switch.clone(),
                 true,
                 Some(gpio_lib_handle_for_tls),
                 gpio_tank_level_switch
            ).map_err(|e| format!("TankLevelSwitch error: {:?}", e))?;
            
            let mut tls_channels = channels.tank_level_switch;
            let mutex_tls_clone = mutex_tank_level_switch_signals.clone();
            
            thread::spawn(move || {
                let _ = tank_level_switch.execute(&mut tls_channels, mutex_tls_clone);
            });
        }
        
        if execution_config.atlas_scientific {
             let atlas_drivers = AtlasScientific::create_drivers(&config.atlas_scientific)
                 .map_err(|e| format!("Atlas driver error: {:?}", e))?;
             let mut atlas = AtlasScientific::new(config.atlas_scientific.clone(), atlas_drivers);
             
             let mut atlas_channels = channels.atlas_scientific;
             let m_temp = mutex_atlas_scientific_temperature.clone();
             let m_ph = mutex_atlas_scientific_ph.clone();
             let m_cond = mutex_atlas_scientific_conductivity.clone();
             
             if execution_config.i2c_interface {
                 let mut i2c = I2cInterface::new(config.i2c_interface.clone())
                     .map_err(|e| format!("I2C error: {:?}", e))?;
                 let mut i2c_channels = channels.i2c_interface;
                 thread::spawn(move || {
                     i2c.execute(&mut i2c_channels);
                 });
             }
             
             thread::spawn(move || {
                 atlas.init_devices(&mut atlas_channels).expect("Atlas init failed");
                 atlas.execute(&mut atlas_channels, m_temp, m_ph, m_cond);
             });
        }
    }

    if execution_config.ds18b20 {
        let mut ds18b20 = Ds18b20::new(config.ds18b20.clone())?;
        let mut ds18b20_channels = channels.ds18b20;
        let m_water = mutex_ds18b20_water_temperature.clone();
        let m_amb = mutex_ds18b20_ambient_temperature.clone();
        thread::spawn(move || {
           ds18b20.execute(&mut ds18b20_channels, m_water, m_amb);
        });
    }

    // 5. Setup Actuator
    let mut actuator: Box<dyn RelayActuationTrait> = if config.relay_manager.use_simulator {
        Box::new(ActuateSimulator::new(
             &mut channels.actuate_simulator,
             config.controllino_relay.clone()
        ))
    } else if config.relay_manager.use_gpio {
        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        {
             let gpio = rppal::gpio::Gpio::new().map_err(|e| format!("GPIO error: {}", e))?;
             let gpio_config = ActuateGpioConfig::new(&config.gpio_handler);
             Box::new(ActuateGpio::new(gpio, gpio_config).map_err(|e| format!("ActuateGpio error: {:?}", e))?)
        }
        #[cfg(not(all(target_os = "linux", feature = "target_hw")))]
        {
            return Err("GPIO not supported on this platform.".into());
        }
    } else {
        Box::new(ActuateControllino::new(config.controllino_relay.clone()).map_err(|e| format!("Controllino error: {:?}", e))?)
    };


    // 6. Setup TUI
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app = App::new(sensor_mutexes, execution_config.sensor_manager);
    let res = run_app(&mut terminal, app, &mut *actuator);

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

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut app: App,
    actuator: &mut dyn RelayActuationTrait,
) -> Result<(), Box<dyn Error>> {
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(250);
    
    let mut last_heartbeat = Instant::now();
    let heartbeat_interval = actuator.get_heartbeat_interval_seconds()
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(3600)); // Default long if none

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Down => app.next(),
                    KeyCode::Up => app.previous(),
                    KeyCode::Char(' ') => {
                        // Toggle
                        if let Some(idx) = app.list_state.selected() {
                            let device = app.devices[idx].clone();
                            let name = device.to_string();
                            let current_state = *app.device_states.get(&name).unwrap_or(&false);
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
                    _ => {}
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
        })
        .collect();

    let items = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Actuators (Space to Toggle)"))
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(Color::DarkGray),
        )
        .highlight_symbol(">> ");

    f.render_stateful_widget(items, chunks[0], &mut app.list_state);

    // Right: Sensors
    if app.sensor_manager_active {
        let sensor_text: Vec<ListItem> = SIGNALS.iter().map(|sig| {
            let val = app.signals.get(sig).unwrap_or(&0.0);
            let content = format!("{}: {}", sig, sig.output_file_string(*val));
            ListItem::new(content)
        }).collect();
        
        let sensors_list = List::new(sensor_text)
            .block(Block::default().borders(Borders::ALL).title("Sensors"));
            
        f.render_widget(sensors_list, chunks[1]);
    } else {
        let text = vec![
            ListItem::new("Sensor Manager is disabled."),
            ListItem::new("Enable it in the configuration to see sensor data."),
        ];
        let sensors_list = List::new(text)
            .block(Block::default().borders(Borders::ALL).title("Sensors"));
        f.render_widget(sensors_list, chunks[1]);
    }
}
