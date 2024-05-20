/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

/// Holds the configuration data of one feed pattern.
pub struct Feedpattern {
    pub profile_id: i32,
    pub profile_name: String,
    pub feedphases: Vec<FeedPhase>,
}

/// Holds the configuration data of one feed phase.
pub struct FeedPhase {
    pub pause_duration: i16,
    pub pause_skimmer: bool,
    pub pause_main_pump_1: bool,
    pub pause_main_pump_2: bool,
    pub pause_aux_pump_1: bool,
    pub pause_aux_pump_2: bool,
    pub feed_duration: i16,
    pub feed_skimmer: bool,
    pub feed_main_pump_1: bool,
    pub feed_main_pump_2: bool,
    pub feed_aux_pump_1: bool,
    pub feed_aux_pump_2: bool,
}
