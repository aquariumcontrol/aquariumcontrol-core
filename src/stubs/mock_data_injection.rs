/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    use crate::database::sql_interface_data::SqlInterfaceData;
    use crate::recorder::{data_injection::DataInjectionTrait, recorder_data_frame::RecorderDataFrame};

    pub struct MockDataInjection {
        pub data_frames: Vec<RecorderDataFrame>,
    }

    impl MockDataInjection {
        pub fn new() -> MockDataInjection {
            MockDataInjection {
                data_frames: Vec::new(),
            }
        }

        pub fn output(&self) {
            if self.data_frames.is_empty() {
                println!("MockDataInjection has not recorded any data frames.");
            }
            for (position, data_frame) in self.data_frames.iter().enumerate() {
                println!("Dataframe #{}: {}", position, data_frame);
            }
        }
    }

    impl DataInjectionTrait for MockDataInjection {
        fn inject_data(&mut self, data_frame: RecorderDataFrame, _: &mut SqlInterfaceData) {
            println!(
                "MockDataInjection.inject_data: recording data frame: {}",
                data_frame
            );

            // add data frame to vector
            self.data_frames.push(data_frame);
        }
    }
}