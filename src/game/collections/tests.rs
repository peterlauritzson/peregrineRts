//! Tests for InclusionSet

#[cfg(test)]
mod tests {
    use super::super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    struct TestId(u32);

    impl From<u32> for TestId {
        fn from(val: u32) -> Self {
            TestId(val)
        }
    }

    impl From<TestId> for usize {
        fn from(val: TestId) -> Self {
            val.0 as usize
        }
    }

    #[test]
    fn test_hot_mode_basic() {
        let config = SetConfig {
            max_capacity: 1000,
            hot_capacity: Some(100),
            hysteresis_buffer: Some(10),
            sorted: false,
        };

        let mut set = InclusionSet::<TestId>::new(config);

        let r1 = set.include(TestId(1));
        let r2 = set.include(TestId(2));
        let r3 = set.include(TestId(3));
        
        // Should all be in hot storage
        assert!(matches!(r1, IncludeResult::Hot(_)));
        assert!(matches!(r2, IncludeResult::Hot(_)));
        assert!(matches!(r3, IncludeResult::Hot(_)));

        assert_eq!(set.count(), 3);
        assert!(set.contains(TestId(1)));

        let items: Vec<_> = set.iter().collect();
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn test_migration_to_bitset() {
        let config = SetConfig {
            max_capacity: 1000,
            hot_capacity: Some(5), // Small to force migration
            hysteresis_buffer: Some(1),
            sorted: false,
        };

        let mut set = InclusionSet::<TestId>::new(config);

        // Add items up to capacity
        for i in 0..5 {
            assert!(matches!(set.include(TestId(i)), IncludeResult::Hot(_)));
        }
        assert_eq!(set.stats().mode, "Hot");

        // This should trigger migration
        let result = set.include(TestId(10));
        assert!(matches!(result, IncludeResult::Bitset));
        assert_eq!(set.stats().mode, "Bitset");
        assert_eq!(set.count(), 6);

        // All items should still be present
        for i in 0..5 {
            assert!(set.contains(TestId(i)));
        }
        assert!(set.contains(TestId(10)));
    }

    #[test]
    fn test_migration_back_to_hot() {
        let config = SetConfig {
            max_capacity: 1000,
            hot_capacity: Some(10),
            hysteresis_buffer: Some(3),
            sorted: false,
        };

        let mut set = InclusionSet::<TestId>::new(config);

        // Fill beyond hot capacity to force bitset mode
        for i in 0..15 {
            set.include(TestId(i));
        }
        assert_eq!(set.stats().mode, "Bitset");

        // Remove items to go below threshold (10 - 3 = 7)
        // In bitset mode, index is ignored
        for i in 0..10 {
            set.exclude(TestId(i), None);
        }
        set.sweep(|_, _| {}); // Should trigger migration back

        assert_eq!(set.stats().mode, "Hot");
        assert_eq!(set.count(), 5);

        // Remaining items should still be present
        for i in 10..15 {
            assert!(set.contains(TestId(i)));
        }
    }

    #[test]
    fn test_hysteresis_prevents_thrashing() {
        let config = SetConfig {
            max_capacity: 1000,
            hot_capacity: Some(10),
            hysteresis_buffer: Some(3), // Threshold = 10 - 3 = 7
            sorted: false,
        };

        let mut set = InclusionSet::<TestId>::new(config);

        // Go to bitset mode (need > 10)
        for i in 0..11 {
            set.include(TestId(i));
        }
        assert_eq!(set.stats().mode, "Bitset");
        assert_eq!(set.count(), 11);

        // Remove 1 item (count becomes 10)
        set.exclude(TestId(0), None);
        assert_eq!(set.count(), 10); // Bitset removes immediately
        set.sweep(|_, _| {});
        assert_eq!(set.stats().mode, "Bitset"); // 10 > 7, should stay in bitset

        // Remove more items but stay above threshold (count becomes 8)
        set.exclude(TestId(1), None);
        set.exclude(TestId(2), None);
        assert_eq!(set.count(), 8);
        set.sweep(|_, _| {});
        assert_eq!(set.stats().mode, "Bitset"); // 8 > 7, still above threshold

        // Remove one more to go TO threshold (count becomes 7)
        set.exclude(TestId(3), None);
        assert_eq!(set.count(), 7);
        set.sweep(|_, _| {});
        assert_eq!(set.stats().mode, "Bitset"); // 7 == 7, still at threshold

        // Remove one more to go BELOW threshold (count becomes 6 < 7)
        set.exclude(TestId(4), None);
        assert_eq!(set.count(), 6);
        set.sweep(|_, _| {});
        assert_eq!(set.stats().mode, "Hot"); // 6 < 7, now migrate back
    }

    #[test]
    fn test_bitset_only_mode() {
        let config = SetConfig {
            max_capacity: 1000,
            hot_capacity: None, // Bitset-only
            hysteresis_buffer: None,
            sorted: false,
        };

        let mut set = InclusionSet::<TestId>::new(config);

        assert!(matches!(set.include(TestId(10)), IncludeResult::Bitset));
        assert!(matches!(set.include(TestId(20)), IncludeResult::Bitset));
        assert!(matches!(set.include(TestId(30)), IncludeResult::Bitset));

        assert_eq!(set.stats().mode, "Bitset");
        assert_eq!(set.count(), 3);

        let mut items: Vec<_> = set.iter().collect();
        items.sort();
        assert_eq!(items, vec![TestId(10), TestId(20), TestId(30)]);
    }

    #[test]
    fn test_bounds_validation() {
        let config = SetConfig {
            max_capacity: 100,
            hot_capacity: Some(10),
            hysteresis_buffer: Some(2),
            sorted: false,
        };

        let mut set = InclusionSet::<TestId>::new(config);

        // Fill to force bitset mode
        for i in 0..15 {
            set.include(TestId(i));
        }

        // These should be rejected (>= max_capacity)
        set.include(TestId(100));
        set.include(TestId(500));

        assert_eq!(set.count(), 15); // Count unchanged
        assert!(!set.contains(TestId(100)));
    }

    #[test]
    fn test_exclude_with_index() {
        let config = SetConfig {
            max_capacity: 1000,
            hot_capacity: Some(100),
            hysteresis_buffer: Some(10),
            sorted: false,
        };

        let mut set = InclusionSet::<TestId>::new(config);

        // Include items and save indices
        let _idx1 = if let IncludeResult::Hot(idx) = set.include(TestId(1)) { idx } else { panic!() };
        let idx2 = if let IncludeResult::Hot(idx) = set.include(TestId(2)) { idx } else { panic!() };
        let _idx3 = if let IncludeResult::Hot(idx) = set.include(TestId(3)) { idx } else { panic!() };

        assert_eq!(set.count(), 3);

        // Exclude with correct index
        assert!(set.exclude(TestId(2), Some(idx2)));
        
        // Track index updates from sweep
        let mut updates = Vec::new();
        set.sweep(|old_idx, new_idx| {
            updates.push((old_idx, new_idx));
        });

        assert_eq!(set.count(), 2);
        assert!(!set.contains(TestId(2)));
        
        // Verify remaining items
        assert!(set.contains(TestId(1)));
        assert!(set.contains(TestId(3)));
        
        // After sweep, item at index 2 (TestId(3)) should have moved to index 1
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0], (2, 1));
    }
}
