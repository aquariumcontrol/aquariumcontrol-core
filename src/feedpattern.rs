pub struct Feedpattern {
    pub profile_id: i32,
    pub profile_name: String,

    pub feedphases: Vec<FeedPhase>,
}

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
