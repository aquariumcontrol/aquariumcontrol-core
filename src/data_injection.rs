/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::sql_interface_data::DataFrame;
use crate::sql_interface_data::SqlInterfaceData;

/// Trait for the execution of data logging to SQL database
/// This trait allows running the main control with a mock implementation for testing.
pub trait DataInjectionTrait {
    /// Transfers the data provided as parameter to SQL database
    /// # Arguments
    /// * `data_frame` - Data to be transferred to SQL database
    /// * `sql_interface_data` - Interface to SQL database
    fn inject_data(&mut self, data_frame: DataFrame, sql_interface_data: &mut SqlInterfaceData);
}

pub struct DataInjection;

impl DataInjectionTrait for DataInjection {
    fn inject_data(&mut self, data_frame: DataFrame, sql_interface_data: &mut SqlInterfaceData) {
        match sql_interface_data.write_data_frame_to_database(&data_frame) {
            Ok(()) => { /* do nothing */ }
            Err(e) => {
                println!("DataLogger: error occurred when writing to SQL database: {e:?}")
            }
        };
    }
}
