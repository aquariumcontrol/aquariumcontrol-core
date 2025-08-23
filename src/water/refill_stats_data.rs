use std::fmt;

/// Contains the data retrieved from the SQL database.
/// It is also used to communicate data to be updated in the SQL database.
pub struct RefillStatsData {
    /// Duration since the last refill operation in seconds
    pub duration_since_last_refill: u64,

    /// Total volume of refills in the last 24 hours in liters
    pub refill_volume_last_24h: f64,

    /// Count of refills in the last 24 hours
    pub refill_count_last_24h: u64,

    /// Total volume of refills in the last hour in liters
    pub refill_volume_last_hour: f64,

    /// Count of refills in the last hour
    pub refill_count_last_hour: u64,
}

impl RefillStatsData {
    /// Creates a new `RefillStatsData` instance, with all its fields initialized to zero.
    ///
    /// This constructor provides a default, empty state for storing historical refill data,
    /// suitable for initialization before actual values are retrieved from the database.
    ///
    /// # Returns
    /// A new `RefillStatsData` struct with all numeric fields set to `0` or `0.0`.
    pub fn new() -> RefillStatsData {
        RefillStatsData {
            duration_since_last_refill: 0,
            refill_volume_last_24h: 0.0,
            refill_count_last_24h: 0,
            refill_volume_last_hour: 0.0,
            refill_count_last_hour: 0,
        }
    }
}

impl fmt::Display for RefillStatsData {
    /// Provides a multi-line, human-readable output of the struct's values for debugging purposes.
    ///
    /// This implementation formats all fields of the `RefillStatsData` struct into a detailed
    /// string representation, summarizing historical refill statistics such as duration since
    /// the last refill, and total volume and count for both the last 24 hours and the last hour.
    /// This is particularly useful for inspecting refill data during development and logging.
    ///
    /// # Arguments
    /// * `f` - A mutable reference to the formatter, as required by the `fmt::Display` trait.
    ///
    /// # Returns
    /// An `Ok(())` on successful formatting.
    ///
    /// # Errors
    /// Returns an `Err` containing a `std::fmt::Error` if an I/O error occurs
    /// while writing to the formatter.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            " {}: Duration since last refill is {} seconds.\n \
            Refill: volume of last 24h is {} L.\n Refill: Count of last 24h is {}.\n \
            Refill: volume of last hour is {} L.\n Refill: Count of last hour is {}.",
            module_path!(),
            self.duration_since_last_refill,
            self.refill_volume_last_24h,
            self.refill_count_last_24h,
            self.refill_volume_last_hour,
            self.refill_count_last_hour
        )
    }
}
