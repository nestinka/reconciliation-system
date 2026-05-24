use crate::types::AgeingBucket;

/// Maps a non-negative age in days to the canonical bucket used by the UI.
pub fn ageing_bucket(days: i64) -> AgeingBucket {
    match days {
        d if d <= 1 => AgeingBucket::ZeroToOne,
        d if d <= 7 => AgeingBucket::TwoToSeven,
        d if d <= 30 => AgeingBucket::EightToThirty,
        _ => AgeingBucket::ThirtyPlus,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn buckets() {
        assert_eq!(ageing_bucket(0), AgeingBucket::ZeroToOne);
        assert_eq!(ageing_bucket(1), AgeingBucket::ZeroToOne);
        assert_eq!(ageing_bucket(2), AgeingBucket::TwoToSeven);
        assert_eq!(ageing_bucket(7), AgeingBucket::TwoToSeven);
        assert_eq!(ageing_bucket(8), AgeingBucket::EightToThirty);
        assert_eq!(ageing_bucket(30), AgeingBucket::EightToThirty);
        assert_eq!(ageing_bucket(31), AgeingBucket::ThirtyPlus);
    }
}
