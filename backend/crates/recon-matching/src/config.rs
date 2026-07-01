#[derive(Debug, Clone)]
pub struct MatchConfig {
    pub version: String,
    pub amount_tolerance_minor: i64,
    pub date_tolerance_days: i64,
    pub fuzzy_threshold: f64,
}

impl MatchConfig {
    /// The pinned default configuration used by the seed and tests.
    /// Algorithm v1.1: adds an exact-identifier counterparty term to scoring.
    /// The constructor name `v1()` is retained (it is the current pinned config);
    /// `version` is the persisted source of truth for `config_version`.
    pub fn v1() -> Self {
        Self {
            version: "v1.1".into(),
            amount_tolerance_minor: 500,
            date_tolerance_days: 2,
            fuzzy_threshold: 0.6,
        }
    }
}
