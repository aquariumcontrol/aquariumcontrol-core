/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests the Inter-Process Communication (IPC) bridge for external control.

cfg_if::cfg_if! {
    if #[cfg(target_os = "linux")] {
        use posixmq::PosixMq;
        use spin_sleep::SpinSleeper;
        use std::{thread, time::Duration};

        use aquarium_control::dispatch::ipc_message::Message;
        use aquarium_control::dispatch::messaging::Messaging;
        use aquarium_control::dispatch::messaging_channels::MessagingChannels;
        use aquarium_control::dispatch::messaging_domain::MessagingDomain;
        use aquarium_control::launch::channels::AquaSender;
        use aquarium_control::launch::channels::Channels;
        use aquarium_control::launch::execution_config::ExecutionConfig;
        use aquarium_control::mocks::mock_message_command_receiver::tests::mock_message_command_receiver;
        use aquarium_control::utilities::channel_content::InternalCommand;
        use aquarium_control::utilities::config::{read_config_file, ConfigData};
        use aquarium_control::utilities::signal_handler_channels::SignalHandlerChannels;

        /// Opens a POSIX message queue for test purposes.
        ///
        /// This helper function is exclusively used in test environments on Linux.
        /// It attempts to open a POSIX message queue specified by its `filename`.
        /// If the message queue cannot be opened, the function will panic,
        /// indicating a critical issue for the test setup.
        ///
        /// # Arguments
        /// * `filename` - The name of the POSIX message queue to open for testing.
        ///
        /// # Returns
        /// An opened `PosixMq` instance.
        ///
        /// # Panics
        /// This function will panic if it fails to open the specified POSIX message queue,
        /// as this is considered a critical failure for the test setup.
        fn open_posixmq_test_file(filename: &str) -> PosixMq {
            match PosixMq::open(filename) {
                Ok(c) => c,
                Err(e) => {
                    panic!(
                        "{}: failure to open message queue for test using {}, error={e:?}",
                        module_path!(),
                        filename
                    );
                }
            }
        }

        /// Creates a new thread to run the `Messaging` module's `execute` loop.
        ///
        /// This helper function is used in test environments on Linux to simulate the `Messaging`
        /// module running in its own dedicated thread. It sets up the necessary channel
        /// connections for communication with various components (like `Refill`, `Heating`, `Watchdog`, etc.).
        ///
        /// # Arguments
        /// * `messaging` - The `Messaging` instance to be executed in the new thread. It is moved into the thread.
        /// * `messaging_channels` - Struct containing the channels for messaging
        ///
        /// # Returns
        /// A `thread::JoinHandle<()>` which can be used to wait for the spawned thread to complete.
        ///
        /// # Panics
        /// This function will panic if the new thread cannot be spawned.
        fn create_test_object(
            mut messaging: Messaging,
            mut messaging_channels: MessagingChannels,
        ) -> thread::JoinHandle<()> {
            thread::Builder::new()
                .name("test_object".to_string())
                .spawn(move || {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap()
                        .block_on(async {
                            // blocking execution used for testing
                            messaging.execute(&mut messaging_channels).await;
                        });
                })
                .unwrap()
        }

        // Creates a new thread to simulate external stimuli and commands for the `Messaging` module.
        //
        // This helper function is used in test environments on Linux. It simulates an
        // external process sending commands to the `Messaging` module via a POSIX message queue
        // and also sends shutdown commands to the `Messaging` module and a mock control object
        // through internal channels.
        //
        // # Arguments
        // * `msg_commands` - A vector of `Message` structs to be sent as stimuli to the `Messaging` module.
        // * `tx_environment_to_object` - The sender channel for this test environment to send commands to a mock control object.
        // * `tx_signal_handler_to_messaging` - The sender channel for this test environment to send commands to the `Messaging` module, simulating the signal handler.
        // * `rx_signal_handler_from_messaging` - The receiver channel for this test environment to receive acknowledgments from the `Messaging` module.
        // * `mq_filename` - The name of the POSIX message queue used for sending stimuli.
        //
        // # Returns
        // A `thread::JoinHandle<()>` which can be used to wait for the spawned thread to complete.
        //
        // # Panics
        // This function will panic if:
        // - The new thread cannot be spawned.
        /// Performs the `create_test_environment` operation.
        ///
        /// # Arguments
        /// * `msg_commands` - A parameter representing `msg_commands`.
        /// * `tx_environment_to_object` - A parameter representing `tx_environment_to_object`.
        /// * `signal_handler_channels` - A parameter representing `signal_handler_channels`.
        /// * `mq_filename` - A parameter representing `mq_filename`.
        fn create_test_environment(
            msg_commands: Vec<Message>,
            mut tx_environment_to_object: AquaSender<InternalCommand>,
            signal_handler_channels: SignalHandlerChannels,
            mq_filename: String,
        ) -> thread::JoinHandle<()> {
            thread::Builder::new()
                .name("test_environment".to_string())
                .spawn(move || {
                    let sleep_duration_100_millis = Duration::from_millis(100);
                    let spin_sleeper = SpinSleeper::default();

                    spin_sleeper.sleep(sleep_duration_100_millis);

                    let mq_stimuli = open_posixmq_test_file(&mq_filename);

                    for msg_command in msg_commands {
                        // sending an IPC message requesting the command to the (sub-)test object
                        let _ = mq_stimuli.send(0, &msg_command.to_bytes());

                        spin_sleeper.sleep(sleep_duration_100_millis);
                    }

                    // requesting mock control to quit
                    let _ = tx_environment_to_object.send(InternalCommand::Quit);

                    let mut tx_signal_handler_to_messaging = signal_handler_channels
                        .tx_signal_handler_to_messaging_opt
                        .unwrap();
                    let mut rx_signal_handler_from_messaging = signal_handler_channels
                        .rx_signal_handler_from_messaging_opt
                        .unwrap();

                    // requesting messaging to quit
                    let _ = tx_signal_handler_to_messaging.send(InternalCommand::Quit);

                    // waiting for confirmation from messaging
                    let _ = rx_signal_handler_from_messaging.blocking_recv();

                    let _ = tx_signal_handler_to_messaging.send(InternalCommand::Terminate);
                })
                .unwrap()
        }

        /// Asserts that a specific sequence of "Stop" then "Start" commands was received.
        ///
        /// This helper function is used in test environments on Linux to validate the order
        /// and content of commands received by a test mock. It specifically checks that:
        /// 1. Exactly two commands were received.
        /// 2. The first command received was `InternalCommand::Stop`.
        /// 3. The second command received was `InternalCommand::Start`.
        ///
        /// # Arguments
        /// * `commands_received` - A `Vec<InternalCommand>` containing the commands that were
        ///   captured by a mock object in the order they were received. This vector is consumed.
        ///
        /// # Panics
        /// This function will panic if:
        /// - The `commands_received` vector does not contain exactly two elements.
        /// - The commands are not `InternalCommand::Stop` followed by `InternalCommand::Start` in that order.
        fn assert_stop_start_sequence(mut commands_received: Vec<InternalCommand>) {
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(Duration::from_millis(100));

            for command in &commands_received {
                println!("assert_stop_start_sequence received command: {command:?}");
            }
            // assert the number of commands sent/received are identical
            assert_eq!(2, commands_received.len());

            // assert content of last message sent from test environment:
            assert_eq!(InternalCommand::Start, commands_received.pop().unwrap());

            // assert the content of the first message sent from the test environment:
            assert_eq!(InternalCommand::Stop, commands_received.pop().unwrap());
        }

        // Test case checks if messages for ventilation control are triggering the corresponding channel communication
        #[test]
        pub fn test_messaging_ventilation() {
            let mut config: ConfigData =
                read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
            config.messaging.mq_filename = "/aquarium_control.5UXp".to_string();
            let mq_filename_for_test_environment = config.messaging.mq_filename.clone();

            let mut msg_start = Message::new();
            msg_start.domain = MessagingDomain::Ventilation.into();
            msg_start.command = InternalCommand::Start.to_numeric();

            let mut msg_stop = Message::new();
            msg_stop.domain = MessagingDomain::Ventilation.into();
            msg_stop.command = InternalCommand::Stop.to_numeric();

            let messaging = Messaging::new(config.messaging, ExecutionConfig::default()).unwrap();

            let channels = Channels::new_for_test();

            let tx_test_environment_to_ventilation =
                channels.messaging.tx_messaging_to_ventilation.clone();

            let join_handle_mock_ventilation = thread::Builder::new()
                .name("mock_ventilation".to_string())
                .spawn(move || {
                    assert_stop_start_sequence(mock_message_command_receiver(
                        channels
                            .ventilation
                            .rx_ventilation_from_messaging_opt
                            .unwrap(),
                    ));
                })
                .unwrap();

            // thread for controlling duration of test run
            let join_handle_test_environment = create_test_environment(
                vec![msg_stop, msg_start], // first stop, then start ventilation
                tx_test_environment_to_ventilation,
                channels.signal_handler,
                mq_filename_for_test_environment,
            );

            // thread for the test object
            let join_handle_test_object = create_test_object(messaging, channels.messaging);

            join_handle_mock_ventilation
                .join()
                .expect("Mock ventilation thread did not finish.");
            join_handle_test_environment
                .join()
                .expect("Test environment thread did not finish.");
            join_handle_test_object
                .join()
                .expect("Test object thread did not finish.");
        }

        // Test case checks if messages for heating control are triggering the corresponding channel communication
        #[test]
        pub fn test_messaging_heating() {
            let mut config: ConfigData =
                read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
            config.messaging.mq_filename = "/aquarium_control.6UXp".to_string();
            let mq_filename_for_test_environment = config.messaging.mq_filename.clone();

            let mut msg_start = Message::new();
            msg_start.domain = MessagingDomain::Heating.into();
            msg_start.command = InternalCommand::Start.to_numeric();

            let mut msg_stop = Message::new();
            msg_stop.domain = MessagingDomain::Heating.into();
            msg_stop.command = InternalCommand::Stop.to_numeric();

            let messaging = Messaging::new(config.messaging, ExecutionConfig::default()).unwrap();

            let channels = Channels::new_for_test();

            let tx_test_environment_to_heating = channels.messaging.tx_messaging_to_heating.clone();

            let join_handle_mock_heating = thread::Builder::new()
                .name("mock_heating".to_string())
                .spawn(move || {
                    assert_stop_start_sequence(mock_message_command_receiver(
                        channels.heating.rx_heating_from_messaging_opt.unwrap(),
                    ));
                })
                .unwrap();

            // thread for controlling duration of test run
            let join_handle_test_environment = create_test_environment(
                vec![msg_stop, msg_start], // first stop, then start heating
                tx_test_environment_to_heating,
                channels.signal_handler,
                mq_filename_for_test_environment,
            );

            // thread for the test object
            let join_handle_test_object = create_test_object(messaging, channels.messaging);

            join_handle_mock_heating
                .join()
                .expect("Mock heating thread did not finish.");
            join_handle_test_environment
                .join()
                .expect("Test environment thread did not finish.");
            join_handle_test_object
                .join()
                .expect("Test object thread did not finish.");
        }

        // Test case checks if messages for refill control are triggering the corresponding channel communication
        #[test]
        pub fn test_messaging_refill() {
            let mut config: ConfigData =
                read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
            config.messaging.mq_filename = "/aquarium_control.7UXp".to_string();
            let mq_file_name_for_test_environment = config.messaging.mq_filename.clone();

            let mut msg_reset = Message::new();
            msg_reset.domain = MessagingDomain::Refill.into();
            msg_reset.command = InternalCommand::ResetAllErrors.to_numeric();

            let mut msg_start = Message::new();
            msg_start.domain = MessagingDomain::Refill.into();
            msg_start.command = InternalCommand::Start.to_numeric();

            let mut msg_stop = Message::new();
            msg_stop.domain = MessagingDomain::Refill.into();
            msg_stop.command = InternalCommand::Stop.to_numeric();

            let messaging = Messaging::new(config.messaging, ExecutionConfig::default()).unwrap();

            let channels = Channels::new_for_test();

            let tx_test_environment_to_refill = channels.messaging.tx_messaging_to_refill.clone();

            let join_handle_mock_refill = thread::Builder::new()
                .name("mock_refill".to_string())
                .spawn(move || {
                    let mut commands_received = mock_message_command_receiver(
                        channels.refill.rx_refill_from_messaging_opt.unwrap(),
                    );

                    // assert the number of commands sent/received are identical
                    assert_eq!(3, commands_received.len());

                    // assert the content of the last message sent from test environment:
                    assert_eq!(InternalCommand::Start, commands_received.pop().unwrap());

                    // assert the content of the second message sent from the test environment:
                    assert_eq!(InternalCommand::Stop, commands_received.pop().unwrap());

                    // assert the content of the first message sent from the test environment:
                    assert_eq!(
                        InternalCommand::ResetAllErrors,
                        commands_received.pop().unwrap()
                    );
                })
                .unwrap();

            // thread for controlling duration of test run
            // thread for controlling duration of test run
            let join_handle_test_environment = create_test_environment(
                vec![msg_reset, msg_stop, msg_start], // first reset errors, then stop refill, then start refill
                tx_test_environment_to_refill,
                channels.signal_handler,
                mq_file_name_for_test_environment,
            );

            // thread for the test object
            let join_handle_test_object = create_test_object(messaging, channels.messaging);

            join_handle_mock_refill
                .join()
                .expect("Mock refill thread did not finish.");
            join_handle_test_environment
                .join()
                .expect("Test environment thread did not finish.");
            join_handle_test_object
                .join()
                .expect("Test object thread did not finish.");
        }

        // Test case checks if messages for feed control are triggering the corresponding channel communication
        #[test]
        pub fn test_messaging_feed() {
            let mut config: ConfigData =
                read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
            config.messaging.mq_filename = "/aquarium_control.8UXp".to_string();
            let mq_filename_for_test_environment = config.messaging.mq_filename.clone();

            let mut msg_exec = Message::new();
            msg_exec.domain = MessagingDomain::Feed.into();
            msg_exec.command = InternalCommand::Execute(13).to_numeric();
            msg_exec.command_param1 = 13;

            let mut msg_start = Message::new();
            msg_start.domain = MessagingDomain::Feed.into();
            msg_start.command = InternalCommand::Start.to_numeric();

            let mut msg_stop = Message::new();
            msg_stop.domain = MessagingDomain::Feed.into();
            msg_stop.command = InternalCommand::Stop.to_numeric();

            let messaging = Messaging::new(config.messaging, ExecutionConfig::default()).unwrap();

            let channels = Channels::new_for_test();

            let tx_test_environment_to_feed = channels.messaging.tx_messaging_to_feed.clone();

            let join_handle_mock_feed = thread::Builder::new()
                .name("mock_feed".to_string())
                .spawn(move || {
                    let mut commands_received = mock_message_command_receiver(
                        channels.feed.rx_feed_from_messaging_opt.unwrap(),
                    );

                    // assert the number of commands sent/received are identical
                    assert_eq!(3, commands_received.len());

                    // assert content of last message sent from test environment:
                    assert_eq!(InternalCommand::Start, commands_received.pop().unwrap());

                    // assert the content of the second message sent from the test environment:
                    assert_eq!(InternalCommand::Stop, commands_received.pop().unwrap());

                    // assert the content of the first message sent from the test environment:
                    assert_eq!(
                        InternalCommand::Execute(13),
                        commands_received.pop().unwrap()
                    );
                })
                .unwrap();

            // thread for controlling duration of test run
            let join_handle_test_environment = create_test_environment(
                vec![msg_exec, msg_stop, msg_start], // first execute a feed profile, then stop feed, then start feed
                tx_test_environment_to_feed,
                channels.signal_handler,
                mq_filename_for_test_environment,
            );

            // thread for the test object
            let join_handle_test_object = create_test_object(messaging, channels.messaging);

            join_handle_mock_feed
                .join()
                .expect("Mock feed thread did not finish.");
            join_handle_test_environment
                .join()
                .expect("Test environment thread did not finish.");
            join_handle_test_object
                .join()
                .expect("Test object thread did not finish.");
        }

        // Test case checks if messages for Balling control are triggering the corresponding channel communication
        #[test]
        pub fn test_messaging_balling() {
            let mut config: ConfigData =
                read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
            config.messaging.mq_filename = "/aquarium_control.9UXp".to_string();
            let mq_filename_for_test_environment = config.messaging.mq_filename.clone();

            open_posixmq_test_file(&config.messaging.mq_filename);

            let mut msg_start = Message::new();
            msg_start.domain = MessagingDomain::Balling.into();
            msg_start.command = InternalCommand::Start.to_numeric();

            let mut msg_stop = Message::new();
            msg_stop.domain = MessagingDomain::Balling.into();
            msg_stop.command = InternalCommand::Stop.to_numeric();

            let messaging = Messaging::new(config.messaging, ExecutionConfig::default()).unwrap();

            let channels = Channels::new_for_test();

            let tx_test_environment_to_balling = channels.messaging.tx_messaging_to_balling.clone();

            let join_handle_mock_balling = thread::Builder::new()
                .name("mock_balling".to_string())
                .spawn(move || {
                    assert_stop_start_sequence(mock_message_command_receiver(
                        channels.balling.rx_balling_from_messaging_opt.unwrap(),
                    ));
                })
                .unwrap();

            // thread for controlling duration of test run
            let join_handle_test_environment = create_test_environment(
                vec![msg_stop, msg_start], // first stop, then start Balling dosing control
                tx_test_environment_to_balling,
                channels.signal_handler,
                mq_filename_for_test_environment,
            );

            // thread for the test object
            let join_handle_test_object = create_test_object(messaging, channels.messaging);

            join_handle_mock_balling
                .join()
                .expect("Mock Balling thread did not finish.");
            join_handle_test_environment
                .join()
                .expect("Test environment thread did not finish.");
            join_handle_test_object
                .join()
                .expect("Test object thread did not finish.");
        }

        // Test case checks if messages for Watchdog control are triggering the corresponding channel communication
        #[test]
        pub fn test_messaging_watchdog() {
            let mut config: ConfigData =
                read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
            config.messaging.mq_filename = "/aquarium_control.9UXp".to_string();
            let mq_filename_for_test_environment = config.messaging.mq_filename.clone();

            let mut msg_start = Message::new();
            msg_start.domain = MessagingDomain::Watchdog.into();
            msg_start.command = InternalCommand::Start.to_numeric();

            let mut msg_stop = Message::new();
            msg_stop.domain = MessagingDomain::Watchdog.into();
            msg_stop.command = InternalCommand::Stop.to_numeric();

            let messaging = Messaging::new(config.messaging, ExecutionConfig::default()).unwrap();

            let channels = Channels::new_for_test();

            let tx_test_environment_to_watchdog = channels.messaging.tx_messaging_to_watchdog.clone();

            let join_handle_mock_watchdog = thread::Builder::new()
                .name("mock_watchdog".to_string())
                .spawn(move || {
                    assert_stop_start_sequence(mock_message_command_receiver(
                        channels.watchdog.rx_watchdog_from_messaging_opt.unwrap(),
                    ));
                })
                .unwrap();

            // thread for controlling duration of test run
            let join_handle_test_environment = create_test_environment(
                vec![msg_stop, msg_start], // first stop, then start Watchdog
                tx_test_environment_to_watchdog,
                channels.signal_handler,
                mq_filename_for_test_environment,
            );

            // thread for the test object
            let join_handle_test_object = create_test_object(messaging, channels.messaging);

            join_handle_mock_watchdog
                .join()
                .expect("Mock Balling thread did not finish.");
            join_handle_test_environment
                .join()
                .expect("Test environment thread did not finish.");
            join_handle_test_object
                .join()
                .expect("Test object thread did not finish.");
        }
    }
}
