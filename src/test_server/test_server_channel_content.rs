/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::utilities::channel_content::{AquariumDevice, AquariumSignal};

/// This enum is used for internal thread messaging from Controllino device simulator to model environment thread.
/// These values are nested into enum InternalCommand. They represent the injection coming from one of the peristaltic pumps which is operated for only very short periods of time.
#[derive(Clone, Debug)]
pub enum BallingFluidInjection {
    Calcium(u32),
    Magnesium(u32),
    #[allow(non_camel_case_types)]
    KH(u32),
    NotConnected,
}

/// This enum is used for internal thread messaging from Controllino device simulator or command parser to model environment thread.
#[derive(Clone, Debug)]
pub enum TestServerInternalCommand {
    SwitchOn(AquariumDevice),
    SwitchOff(AquariumDevice),
    RequestSignal(AquariumSignal),
    Inject(BallingFluidInjection),
    Quit,
}
