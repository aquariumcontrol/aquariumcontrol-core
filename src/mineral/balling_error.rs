/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::database::sql_interface_error::SqlInterfaceError;
use std::num::TryFromIntError;
use thiserror::Error;

/// Contains error definitions for Balling
#[derive(Debug, Error)]
pub enum BallingError {
    /// Balling check interval is zero.
    #[error("schedule check interval is zero - please assign a value > 0")]
    ScheduleCheckIntervalZero,

    /// Could not get set values for a pump from database.
    #[error("Could not get set values for pump {pump_id} from database.")]
    SetValueRetrievalError {
        pump_id: u64,

        #[source]
        source: SqlInterfaceError,
    },

    /// Invalid dosing interval in configuration file
    #[error("Invalid dosing interval for pump {0} in configuration file: Dosing interval ({1}) is shorter than check interval ({2}).")]
    DosingIntervalShorterThanCheckInterval(u32, u32, u32),

    /// Countdown calculation encountered invalid pump id.
    #[error("Countdown calculation encountered invalid pump id {0}")]
    CountdownCalculationInvalidPumpId(i64),

    /// Invalid dosing interval in configuration.
    #[error("Invalid dosing interval for pump #{0} in configuration.")]
    InvalidDosingInterval(i64),

    /// Calculating duration since the last dosing event failed.
    #[error("Calculating duration since last dosing event for pump {pump_id} failed.")]
    CalculateDurationSinceLastDosingFailure {
        pump_id: i64,

        #[source]
        source: TryFromIntError,
    },

    /// Reading duration since the last dosing event from the database failed.
    #[error("Reading duration since last dosing event for pump {pump_id} from database failed.")]
    ReadDurationSinceLastDosingFailure {
        pump_id: i64,

        #[source]
        source: SqlInterfaceError,
    },
}
