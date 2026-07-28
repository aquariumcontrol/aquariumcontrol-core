/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

/// A macro to perform a request-response check with the schedule_check module.
///
/// This macro encapsulates the logic for:
/// 1. Sending a `ScheduleCheck` command.
/// 2. Receiving a boolean response.
/// 3. Handling send/receive errors with a "log-once" pattern.
/// 4. Assigning the result, defaulting to `false` on any communication failure.
#[macro_export]
macro_rules! perform_schedule_check {
    (
        $channels:expr,
        $result_var:expr,
        $send_lock:expr,
        $receive_lock:expr,
        $location:expr
    ) => {
        match $channels.send_to_schedule_check($crate::utilities::channel_content::InternalCommand::ScheduleCheck) {
            Ok(_) => {
                $send_lock = false;
                // Now listen for the response
                $result_var = match $channels.receive_from_schedule_check().await {
                    Ok(c) => {
                        $receive_lock = false;
                        c
                    }
                    Err(e) => {
                        // [impl->swreq~mineral_log_flood_protection~1]
                        // [impl->swreq~thermal_log_flood_protection~1]
                        // [impl->swreq~refill_log_flood_protection~1]
                        if !$receive_lock {
                            log::error!(
                                target: $location,
                                "Receiving from schedule check failed ({e:?})"
                            );
                            $receive_lock = true;
                        }
                        // Assume allowed to actuate if communication fails
                        true
                    }
                };
            }
            Err(e) => {
                // [impl->swreq~mineral_log_flood_protection~1]
                // [impl->swreq~thermal_log_flood_protection~1]
                // [impl->swreq~refill_log_flood_protection~1]
                if !$send_lock {
                    log::error!(
                        target: $location,
                        "Channel communication to schedule check failed ({e:?})"
                    );
                    $send_lock = true;
                }
                // On send failure, also assume allowed to actuate
                $result_var = true;
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::launch::channels::{channel, AquaChannelError, AquaReceiver, AquaSender};
    use crate::utilities::channel_content::InternalCommand;

    struct MockScheduleChannels {
        tx: AquaSender<InternalCommand>,
        rx: AquaReceiver<bool>,
    }

    impl MockScheduleChannels {
        fn send_to_schedule_check(&mut self, cmd: InternalCommand) -> Result<(), AquaChannelError> {
            self.tx.send(cmd)
        }

        async fn receive_from_schedule_check(&mut self) -> Result<bool, AquaChannelError> {
            self.rx.recv().await
        }
    }

    // [utest->swreq~mineral_log_flood_protection~1]
    // [utest->swreq~thermal_log_flood_protection~1]
    // [utest->swreq~water_log_flood_protection~1]
    #[tokio::test]
    async fn test_perform_schedule_check_macro_happy_path() {
        let (tx_cmd, mut rx_cmd) = channel::<InternalCommand>(10, false);
        let (mut tx_resp, rx_resp) = channel::<bool>(10, false);

        let mut channels = MockScheduleChannels {
            tx: tx_cmd,
            rx: rx_resp,
        };

        tokio::spawn(async move {
            if let Ok(InternalCommand::ScheduleCheck) = rx_cmd.recv().await {
                tx_resp.send(false).unwrap();
            }
        });

        let result_var;
        let mut send_lock = true;
        let mut receive_lock = true;

        perform_schedule_check!(
            channels,
            result_var,
            send_lock,
            receive_lock,
            "test_perform_schedule_check_happy_path"
        );

        assert!(!result_var, "Schedule check result should be false");
        assert!(
            !send_lock,
            "Send lock flag should be reset to false on success"
        );
        assert!(
            !receive_lock,
            "Receive lock flag should be reset to false on success"
        );
    }

    // [utest->swreq~mineral_log_flood_protection~1]
    // [utest->swreq~thermal_log_flood_protection~1]
    // [utest->swreq~water_log_flood_protection~1]
    #[tokio::test]
    async fn test_perform_schedule_check_macro_send_error_and_log_flood_protection() {
        let (tx_cmd, rx_cmd) = channel::<InternalCommand>(10, false);
        let (_tx_resp, rx_resp) = channel::<bool>(10, false);
        drop(rx_cmd); // Disconnect send channel to trigger send error

        let mut channels = MockScheduleChannels {
            tx: tx_cmd,
            rx: rx_resp,
        };

        let result_var;
        let mut send_lock = false;
        let mut receive_lock = false;

        perform_schedule_check!(
            channels,
            result_var,
            send_lock,
            receive_lock,
            "test_perform_schedule_check_send_error"
        );

        assert!(
            result_var,
            "Result should default to true on communication error"
        );
        assert!(
            send_lock,
            "Send lock should be set to true on first send error"
        );
        assert!(!receive_lock, "Receive lock should remain false");

        let result_var_2;
        perform_schedule_check!(
            channels,
            result_var_2,
            send_lock,
            receive_lock,
            "test_perform_schedule_check_send_error"
        );

        assert!(
            result_var_2,
            "Result should default to true on communication error"
        );
        assert!(
            send_lock,
            "Send lock should remain true to suppress repeated error logs"
        );
        assert!(!receive_lock, "Receive lock should remain false");
    }

    // [utest->swreq~mineral_log_flood_protection~1]
    // [utest->swreq~thermal_log_flood_protection~1]
    // [utest->swreq~water_log_flood_protection~1]
    #[tokio::test]
    async fn test_perform_schedule_check_macro_receive_error_and_log_flood_protection() {
        let (tx_cmd, mut rx_cmd) = channel::<InternalCommand>(10, false);
        let (tx_resp, rx_resp) = channel::<bool>(10, false);
        drop(tx_resp); // Disconnect receive channel so recv().await fails

        let mut channels = MockScheduleChannels {
            tx: tx_cmd,
            rx: rx_resp,
        };

        tokio::spawn(async move {
            let _ = rx_cmd.recv().await;
        });

        let result_var;
        let mut send_lock = false;
        let mut receive_lock = false;

        perform_schedule_check!(
            channels,
            result_var,
            send_lock,
            receive_lock,
            "test_perform_schedule_check_recv_error"
        );

        assert!(result_var, "Result should default to true on receive error");
        assert!(!send_lock, "Send lock should be false as send succeeded");
        assert!(
            receive_lock,
            "Receive lock should be set to true on first receive error"
        );

        let result_var_2;
        perform_schedule_check!(
            channels,
            result_var_2,
            send_lock,
            receive_lock,
            "test_perform_schedule_check_recv_error"
        );

        assert!(
            result_var_2,
            "Result should default to true on receive error"
        );
        assert!(!send_lock, "Send lock should remain false");
        assert!(
            receive_lock,
            "Receive lock should remain true to suppress repeated error logs"
        );
    }
}
