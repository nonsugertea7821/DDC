#[derive(Debug, Clone)]
pub struct Timer {
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone)]
pub struct Alarm {
    pub label: String,
    pub threshold_ms: u64,
    pub fired: bool,
}
