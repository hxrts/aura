//! Demo mode flow tests.
//!
//! Verify that demo flows exercise production code paths with simulated
//! effects — shortcuts or divergence from production would invalidate
//! demo-based testing.

mod demo {
    mod demo_amp_channel_echo;
    mod demo_device_enrollment_flow;
    mod demo_device_removal_flow;
    mod demo_echo_regression;
    mod demo_hints;
    mod demo_invitation_flow;
    mod demo_multi_device_enrollment_flow;
    mod demo_peer_count;
}
