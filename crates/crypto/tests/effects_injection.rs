#![allow(warnings, clippy::all)]
//! Unit Tests: Injected Effects
//!
//! Tests that the Effects system provides deterministic time and randomness for testing.
//! Every SSB and Storage test depends on these working correctly.
//!
//! Reference: work/pre_ssb_storage_tests.md - Category 7.1

use aura_test_utils::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_time() {
        // Create Effects with fixed time (2025-01-01 00:00:00 UTC)
        let initial_time = 1735689600;
        let effects = test_effects_deterministic(42, initial_time);

        // Call effects.now() multiple times
        let time1 = effects.now().expect("Getting time should succeed");
        let time2 = effects.now().expect("Getting time should succeed");
        let time3 = effects.now().expect("Getting time should succeed");

        // Assert: Time does not advance unless explicitly advanced
        assert_eq!(
            time1, initial_time,
            "Initial time should match configured value"
        );
        assert_eq!(time1, time2, "Time should not advance automatically");
        assert_eq!(time2, time3, "Time should remain fixed");

        // Explicitly advance time by 3600 seconds (1 hour)
        effects
            .advance_time(3600)
            .expect("Time advancement should succeed");

        let time4 = effects.now().expect("Getting time should succeed");
        assert_eq!(
            time4,
            initial_time + 3600,
            "Time should advance when explicitly requested"
        );

        // Multiple reads after advancement should still be stable
        let time5 = effects.now().expect("Getting time should succeed");
        assert_eq!(time4, time5, "Time should remain stable after advancement");

        // Jump to specific time (time-travel)
        let future_time = initial_time + 86400; // +1 day
        effects
            .set_time(future_time)
            .expect("Time travel should succeed");

        let time6 = effects.now().expect("Getting time should succeed");
        assert_eq!(
            time6, future_time,
            "Time should jump to specified timestamp"
        );

        println!("[OK] test_deterministic_time PASSED");
    }

    #[test]
    fn test_deterministic_randomness() {
        // Create Effects with seed 42
        let seed = 42;
        let effects1 = test_effects_deterministic(seed, 0);

        // Generate random bytes twice from first instance
        let bytes1a: [u8; 32] = effects1.random_bytes();
        let bytes1b: [u8; 32] = effects1.random_bytes();

        // Assert: Sequential calls produce different values (RNG is advancing)
        assert_ne!(
            bytes1a, bytes1b,
            "Sequential random calls should produce different values"
        );

        // Create new Effects with same seed
        let effects2 = test_effects_deterministic(seed, 0);

        // Generate random bytes from second instance
        let bytes2a: [u8; 32] = effects2.random_bytes();
        let bytes2b: [u8; 32] = effects2.random_bytes();

        // Assert: Same seed produces same randomness sequence
        assert_eq!(
            bytes1a, bytes2a,
            "Same seed should produce same first random value"
        );
        assert_eq!(
            bytes1b, bytes2b,
            "Same seed should produce same second random value"
        );

        // Create Effects with different seed
        let effects3 = test_effects_deterministic(999, 0);
        let bytes3: [u8; 32] = effects3.random_bytes();

        // Assert: Different seed produces different values
        assert_ne!(
            bytes1a, bytes3,
            "Different seed should produce different random values"
        );

        println!("[OK] test_deterministic_randomness PASSED");
    }

    #[test]
    fn test_deterministic_uuid_generation() {
        // Create Effects with seed
        let seed = 12345;
        let effects1 = test_effects_deterministic(seed, 0);

        // Generate 10 UUIDs
        let uuids1: Vec<_> = (0..10).map(|_| effects1.gen_uuid()).collect();

        // Assert: All UUIDs are unique
        for i in 0..uuids1.len() {
            for j in (i + 1)..uuids1.len() {
                assert_ne!(
                    uuids1[i], uuids1[j],
                    "UUIDs should be unique within sequence"
                );
            }
        }

        // Recreate Effects with same seed
        let effects2 = test_effects_deterministic(seed, 0);

        // Generate 10 UUIDs
        let uuids2: Vec<_> = (0..10).map(|_| effects2.gen_uuid()).collect();

        // Assert: UUID sequences match
        for i in 0..10 {
            assert_eq!(
                uuids1[i], uuids2[i],
                "UUID at index {} should match with same seed",
                i
            );
        }

        // Create Effects with different seed
        let effects3 = test_effects_deterministic(67890, 0);
        let uuid3 = effects3.gen_uuid();

        // Assert: Different seed produces different UUID
        assert_ne!(
            uuids1[0], uuid3,
            "Different seed should produce different UUID"
        );

        println!("[OK] test_deterministic_uuid_generation PASSED");
    }

    #[test]
    fn test_effects_test_helper() {
        // Test the convenience test() constructor
        let effects = test_effects();

        // Should be deterministic (simulated)
        assert!(
            effects.is_simulated(),
            "test_effects_test() should be simulated"
        );

        // Should have consistent behavior
        let time1 = effects.now().expect("Getting time should succeed");
        let time2 = effects.now().expect("Getting time should succeed");
        assert_eq!(time1, time2, "Test effects should have stable time");

        // Should produce deterministic randomness
        let random1: [u8; 16] = effects.random_bytes();
        let effects_test2 = test_effects();
        let random2: [u8; 16] = effects_test2.random_bytes();
        assert_eq!(
            random1, random2,
            "test_effects_test() should use same default seed"
        );

        println!("[OK] test_effects_test_helper PASSED");
    }

    #[test]
    fn test_effects_production_is_not_simulated() {
        // Production effects should NOT be simulated
        let effects = test_effects_production();

        assert!(
            !effects.is_simulated(),
            "Production effects should not be simulated"
        );

        // Time should advance naturally
        let time1 = effects.now().expect("Getting time should succeed");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let time2 = effects.now().expect("Getting time should succeed");

        // In production, time should have advanced (or at least not gone backwards)
        assert!(time2 >= time1, "Production time should advance naturally");

        // Attempting to manually advance time should fail or be a no-op
        // (production time source doesn't support manual advancement)
        let advance_result = effects.advance_time(1000);
        // It's OK if this succeeds as no-op, but time shouldn't actually jump
        if advance_result.is_ok() {
            let time3 = effects.now().expect("Getting time should succeed");
            // Time should not have jumped by 1000 seconds
            assert!(
                time3 < time2 + 500,
                "Production time should not jump from manual advancement"
            );
        }

        println!("[OK] test_effects_production_is_not_simulated PASSED");
    }

    #[test]
    fn test_effects_for_test_isolation() {
        // Create effects for two different test names
        let effects_a = test_effects_named("test_a");
        let effects_b = test_effects_named("test_b");

        // Generate random bytes from each
        let random_a: [u8; 32] = effects_a.random_bytes();
        let random_b: [u8; 32] = effects_b.random_bytes();

        // Different test names should produce different randomness
        // (they use hashed test names as seeds)
        assert_ne!(
            random_a, random_b,
            "Different test names should produce different random sequences"
        );

        // Same test name should produce same randomness
        let effects_a2 = test_effects_named("test_a");
        let random_a2: [u8; 32] = effects_a2.random_bytes();
        assert_eq!(
            random_a, random_a2,
            "Same test name should produce same random sequence"
        );

        println!("[OK] test_effects_for_test_isolation PASSED");
    }

    #[test]
    fn test_fill_random_buffer() {
        let effects = test_effects_deterministic(777, 0);

        // Fill a buffer with random bytes
        let mut buffer1 = vec![0u8; 64];
        effects.fill_random(&mut buffer1);

        // Buffer should not be all zeros
        assert_ne!(
            buffer1,
            vec![0u8; 64],
            "Buffer should be filled with random data"
        );

        // Create new effects with same seed
        let effects2 = test_effects_deterministic(777, 0);
        let mut buffer2 = vec![0u8; 64];
        effects2.fill_random(&mut buffer2);

        // Buffers should match (deterministic)
        assert_eq!(
            buffer1, buffer2,
            "Same seed should produce same random buffer"
        );

        println!("[OK] test_fill_random_buffer PASSED");
    }

    #[test]
    fn test_gen_session_id_deterministic() {
        let effects = test_effects_deterministic(888, 0);

        // Generate session IDs
        let session1 = effects.gen_session_id();
        let session2 = effects.gen_session_id();

        // Session IDs should be different
        assert_ne!(session1, session2, "Session IDs should be unique");

        // Recreate with same seed
        let effects2 = test_effects_deterministic(888, 0);
        let session1_replay = effects2.gen_session_id();
        let session2_replay = effects2.gen_session_id();

        // Session IDs should be deterministically reproduced
        assert_eq!(
            session1, session1_replay,
            "Session ID should be deterministic"
        );
        assert_eq!(
            session2, session2_replay,
            "Session ID sequence should be deterministic"
        );

        println!("[OK] test_gen_session_id_deterministic PASSED");
    }
}
