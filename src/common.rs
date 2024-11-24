/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    pub fn update_min_max_actuation_duration(
        min_actuation_duration: u128,
        max_actuation_duration: u128,
        new_actuation_duration: u128,
    ) -> (u128, u128) {
        let mut result_min_actuation_duration: u128 = min_actuation_duration;
        let mut result_max_actuation_duration: u128 = max_actuation_duration;
        if new_actuation_duration < min_actuation_duration {
            result_min_actuation_duration = new_actuation_duration;
        }
        if new_actuation_duration > max_actuation_duration {
            result_max_actuation_duration = new_actuation_duration;
        }
        (result_min_actuation_duration, result_max_actuation_duration)
    }
}
