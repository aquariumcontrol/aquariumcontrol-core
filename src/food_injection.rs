/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
/*
use chrono::Local;
use spin_sleep::SpinSleeper;
use std::time::Duration;

use crate::channel_content::AquariumDevice;
*/
use crate::channel_content::InternalCommand;
use crate::feed::FeedConfig;
use crate::feedpattern::Feedpattern;

/// Trait for the execution of the feed pattern.
/// This trait allows running the main control with a mock implementation for testing.
pub trait FoodInjectionTrait {
    fn inject_food(
        &mut self,
        mutex_device_scheduler_feed: &Arc<Mutex<i32>>,
        tx_to_controllino: &Sender<InternalCommand>,
        rx_from_controllino: &Receiver<bool>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        config: &FeedConfig,
        feed_pattern: &Feedpattern,
    );
}

/// Implementation of the FoodInjectionTrait which contains the code for executing the feed pattern
pub struct FoodInjection;

impl FoodInjectionTrait for FoodInjection {
    fn inject_food(
        &mut self,
        _mutex_device_scheduler_feed: &Arc<Mutex<i32>>,
        _tx_to_controllino: &Sender<InternalCommand>,
        _rx_from_controllino: &Receiver<bool>,
        _rx_from_signal_handler: &Receiver<InternalCommand>,
        _config: &FeedConfig,
        _feed_pattern: &Feedpattern,
    ) {
        // do nothing (yet)
    }
}
