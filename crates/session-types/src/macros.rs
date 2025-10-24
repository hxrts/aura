//! Declarative macros for reducing session type boilerplate
//!
//! This module provides macros to dramatically reduce the verbosity of defining
//! session-typed protocols. These macros generate the repetitive trait implementations
//! while maintaining full compile-time type safety.
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use session_types::macros::*;
//!
//! // Define all states for a protocol (was ~60 lines, now ~10)
//! define_session_states! {
//!     InitializationPhase,
//!     CommitmentPhase,
//!     RevealPhase,
//!     FinalizationPhase,
//!     @final CompletionPhase,
//!     @final Failure,
//! }
//!
//! // Implement SessionProtocol for all states (was ~150 lines, now ~20)
//! impl_session_protocol! {
//!     for DkdProtocol<Core = DkdProtocolCore, Error = DkdSessionError> {
//!         InitializationPhase => (),
//!         CommitmentPhase => [u8; 32],
//!         RevealPhase => Vec<u8>,
//!         FinalizationPhase => (),
//!         CompletionPhase => Vec<u8>,
//!         Failure => (),
//!     }
//!
//!     session_id: |core| core.session_id,
//!     device_id: |core| core.device_id,
//! }
//!
//! // Generate union type with delegation (was ~80 lines, now ~10)
//! define_session_union! {
//!     pub enum DkdProtocolState for DkdProtocolCore {
//!         InitializationPhase,
//!         CommitmentPhase,
//!         RevealPhase,
//!         FinalizationPhase,
//!         CompletionPhase,
//!         Failure,
//!     }
//!
//!     delegate: [state_name, can_terminate, is_final, protocol_id, device_id]
//! }
//! ```

/// Define multiple session states with minimal boilerplate
///
/// This macro generates:
/// - A struct for each state with `#[derive(Debug, Clone)]`
/// - A `SessionState` trait implementation with correct `NAME`
/// - Correct `IS_FINAL` and `CAN_TERMINATE` flags for terminal states
///
/// # Syntax
///
/// ```rust,ignore
/// define_session_states! {
///     // Non-terminal states (default: IS_FINAL=false, CAN_TERMINATE=false)
///     StateName1,
///     StateName2,
///
///     // Terminal states (IS_FINAL=true, CAN_TERMINATE=true)
///     @final TerminalState1,
///     @final TerminalState2,
/// }
/// ```
///
/// # Example
///
/// ```rust,ignore
/// define_session_states! {
///     InitializationPhase,
///     ProcessingPhase,
///     @final CompletionPhase,
///     @final Failure,
/// }
/// ```
///
/// Expands to:
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// pub struct InitializationPhase;
///
/// impl SessionState for InitializationPhase {
///     const NAME: &'static str = "InitializationPhase";
///     const IS_FINAL: bool = false;
///     const CAN_TERMINATE: bool = false;
/// }
///
/// #[derive(Debug, Clone)]
/// pub struct ProcessingPhase;
///
/// impl SessionState for ProcessingPhase {
///     const NAME: &'static str = "ProcessingPhase";
///     const IS_FINAL: bool = false;
///     const CAN_TERMINATE: bool = false;
/// }
///
/// #[derive(Debug, Clone)]
/// pub struct CompletionPhase;
///
/// impl SessionState for CompletionPhase {
///     const NAME: &'static str = "CompletionPhase";
///     const IS_FINAL: bool = true;
///     const CAN_TERMINATE: bool = true;
/// }
///
/// #[derive(Debug, Clone)]
/// pub struct Failure;
///
/// impl SessionState for Failure {
///     const NAME: &'static str = "Failure";
///     const IS_FINAL: bool = true;
///     const CAN_TERMINATE: bool = true;
/// }
/// ```
#[macro_export]
macro_rules! define_session_states {
    // Entry point: process all state definitions
    ( $( $state:ident $(@ $flag:ident )? ),+ $(,)? ) => {
        $(
            define_session_states!(@impl $state $(@ $flag )? );
        )+
    };

    // Implementation for non-terminal states
    (@impl $state:ident) => {
        #[derive(Debug, Clone)]
        pub struct $state;

        impl $crate::core::SessionState for $state {
            const NAME: &'static str = stringify!($state);
            const IS_FINAL: bool = false;
            const CAN_TERMINATE: bool = false;
        }
    };

    // Implementation for terminal states (marked with @final)
    (@impl $state:ident @ final) => {
        #[derive(Debug, Clone)]
        pub struct $state;

        impl $crate::core::SessionState for $state {
            const NAME: &'static str = stringify!($state);
            const IS_FINAL: bool = true;
            const CAN_TERMINATE: bool = true;
        }
    };
}

/// Implement SessionProtocol trait for all states of a protocol
///
/// This macro generates `SessionProtocol` trait implementations for each state,
/// eliminating the need to write repetitive impl blocks.
///
/// # Syntax
///
/// ```rust,ignore
/// impl_session_protocol! {
///     for ProtocolName<Core = CoreType, Error = ErrorType> {
///         State1 => OutputType1,
///         State2 => OutputType2,
///         // ... more states
///     }
///
///     // How to extract session_id from core
///     session_id: |core| core.session_id,
///
///     // How to extract device_id from core
///     device_id: |core| core.device_id,
/// }
/// ```
///
/// # Example
///
/// ```rust,ignore
/// impl_session_protocol! {
///     for DkdProtocol<Core = DkdProtocolCore, Error = DkdSessionError> {
///         InitializationPhase => (),
///         CommitmentPhase => [u8; 32],
///         RevealPhase => Vec<u8>,
///         FinalizationPhase => (),
///         CompletionPhase => Vec<u8>,
///         Failure => (),
///     }
///
///     session_id: |core| core.session_id,
///     device_id: |core| core.device_id,
/// }
/// ```
///
/// Expands to multiple impl blocks like:
///
/// ```rust,ignore
/// impl SessionProtocol for ChoreographicProtocol<DkdProtocolCore, InitializationPhase> {
///     type State = InitializationPhase;
///     type Output = ();
///     type Error = DkdSessionError;
///
///     fn session_id(&self) -> Uuid {
///         self.inner.session_id
///     }
///
///     fn state_name(&self) -> &'static str {
///         InitializationPhase::NAME
///     }
///
///     fn can_terminate(&self) -> bool {
///         InitializationPhase::CAN_TERMINATE
///     }
///
///     fn protocol_id(&self) -> Uuid {
///         self.inner.session_id
///     }
///
///     fn device_id(&self) -> aura_journal::DeviceId {
///         self.inner.device_id
///     }
/// }
/// // ... repeated for each state
/// ```
#[macro_export]
macro_rules! impl_session_protocol {
    (
        for $protocol:ident<Core = $core:ty, Error = $error:ty> {
            $( $state:ident => $output:ty ),+ $(,)?
        }

        session_id: |$core_var1:ident| $session_id_expr:expr,
        device_id: |$core_var2:ident| $device_id_expr:expr $(,)?
    ) => {
        $(
            impl $crate::core::SessionProtocol for $crate::core::ChoreographicProtocol<$core, $state> {
                type State = $state;
                type Output = $output;
                type Error = $error;

                fn session_id(&self) -> ::uuid::Uuid {
                    let $core_var1 = &self.inner;
                    $session_id_expr
                }

                fn state_name(&self) -> &'static str {
                    $state::NAME
                }

                fn can_terminate(&self) -> bool {
                    $state::CAN_TERMINATE
                }

                fn protocol_id(&self) -> ::uuid::Uuid {
                    self.session_id()
                }

                fn device_id(&self) -> ::aura_journal::DeviceId {
                    let $core_var2 = &self.inner;
                    $device_id_expr
                }
            }
        )+
    };
}

/// Define a union type for all states of a protocol with automatic delegation
///
/// This macro generates:
/// - An enum with variants for each state
/// - Delegating methods that match on the enum and call the inner protocol
///
/// # Syntax
///
/// ```rust,ignore
/// define_session_union! {
///     pub enum UnionTypeName for CoreType {
///         State1,
///         State2,
///         // ... more states
///     }
///
///     delegate: [method1, method2, ...]
/// }
/// ```
///
/// # Example
///
/// ```rust,ignore
/// define_session_union! {
///     pub enum DkdProtocolState for DkdProtocolCore {
///         InitializationPhase,
///         CommitmentPhase,
///         RevealPhase,
///         FinalizationPhase,
///         CompletionPhase,
///         Failure,
///     }
///
///     delegate: [state_name, can_terminate, is_final, protocol_id, device_id]
/// }
/// ```
///
/// Expands to:
///
/// ```rust,ignore
/// pub enum DkdProtocolState {
///     InitializationPhase(ChoreographicProtocol<DkdProtocolCore, InitializationPhase>),
///     CommitmentPhase(ChoreographicProtocol<DkdProtocolCore, CommitmentPhase>),
///     RevealPhase(ChoreographicProtocol<DkdProtocolCore, RevealPhase>),
///     FinalizationPhase(ChoreographicProtocol<DkdProtocolCore, FinalizationPhase>),
///     CompletionPhase(ChoreographicProtocol<DkdProtocolCore, CompletionPhase>),
///     Failure(ChoreographicProtocol<DkdProtocolCore, Failure>),
/// }
///
/// impl DkdProtocolState {
///     pub fn state_name(&self) -> &'static str {
///         match self {
///             DkdProtocolState::InitializationPhase(p) => p.state_name(),
///             DkdProtocolState::CommitmentPhase(p) => p.state_name(),
///             DkdProtocolState::RevealPhase(p) => p.state_name(),
///             DkdProtocolState::FinalizationPhase(p) => p.state_name(),
///             DkdProtocolState::CompletionPhase(p) => p.state_name(),
///             DkdProtocolState::Failure(p) => p.state_name(),
///         }
///     }
///
///     // ... similar for can_terminate, is_final, protocol_id, device_id
/// }
/// ```
#[macro_export]
macro_rules! define_session_union {
    (
        $vis:vis enum $enum_name:ident for $core:ty {
            $( $state:ident ),+ $(,)?
        }

        delegate: [ $( $method:ident ),+ $(,)? ]
    ) => {
        // Generate the enum definition
        #[derive(Debug)]
        $vis enum $enum_name {
            $(
                $state($crate::core::ChoreographicProtocol<$core, $state>),
            )+
        }

        // Generate delegating methods
        impl $enum_name {
            // Generate each method separately to avoid repetition issues
            define_session_union!(@generate_methods $enum_name, [ $( $method ),+ ], [ $( $state ),+ ]);
        }
    };

    // Generate all methods
    (@generate_methods $enum_name:ident, [ $( $method:ident ),+ ], [ $( $state:ident ),+ ]) => {
        // Generate each method individually to avoid repetition conflicts
        define_session_union!(@delegate_all $enum_name, $( $state ),+);
    };

    // Generate all delegating methods
    (@delegate_all $enum_name:ident, $( $state:ident ),+) => {
        pub fn state_name(&self) -> &'static str {
            match self {
                $(
                    $enum_name::$state(p) => p.state_name(),
                )+
            }
        }

        pub fn can_terminate(&self) -> bool {
            match self {
                $(
                    $enum_name::$state(p) => p.can_terminate(),
                )+
            }
        }

        pub fn protocol_id(&self) -> ::uuid::Uuid {
            match self {
                $(
                    $enum_name::$state(p) => p.protocol_id(),
                )+
            }
        }

        pub fn device_id(&self) -> ::aura_journal::DeviceId {
            match self {
                $(
                    $enum_name::$state(p) => p.device_id(),
                )+
            }
        }

        pub fn session_id(&self) -> ::uuid::Uuid {
            match self {
                $(
                    $enum_name::$state(p) => p.session_id(),
                )+
            }
        }

        pub fn is_final(&self) -> bool {
            match self {
                $(
                    $enum_name::$state(_p) => $state::IS_FINAL,
                )+
            }
        }
    };

    // Delegate: state_name() -> &'static str
    (@delegate_method state_name, $enum_name:ident, $( $state:ident ),+) => {
        pub fn state_name(&self) -> &'static str {
            match self {
                $(
                    $enum_name::$state(p) => p.state_name(),
                )+
            }
        }
    };

    // Delegate: current_state_name() -> &'static str (alias)
    (@delegate_method current_state_name, $enum_name:ident, $( $state:ident ),+) => {
        pub fn current_state_name(&self) -> &'static str {
            match self {
                $(
                    $enum_name::$state(p) => p.current_state_name(),
                )+
            }
        }
    };

    // Delegate: can_terminate() -> bool
    (@delegate_method can_terminate, $enum_name:ident, $( $state:ident ),+) => {
        pub fn can_terminate(&self) -> bool {
            match self {
                $(
                    $enum_name::$state(p) => p.can_terminate(),
                )+
            }
        }
    };

    // Note: is_final() is not implemented in our session types
    // If needed in the future, check if state IS_FINAL const is true

    // Delegate: protocol_id() -> Uuid
    (@delegate_method protocol_id, $enum_name:ident, $( $state:ident ),+) => {
        pub fn protocol_id(&self) -> ::uuid::Uuid {
            match self {
                $(
                    $enum_name::$state(p) => p.protocol_id(),
                )+
            }
        }
    };

    // Delegate: session_id() -> Uuid
    (@delegate_method session_id, $enum_name:ident, $( $state:ident ),+) => {
        pub fn session_id(&self) -> ::uuid::Uuid {
            match self {
                $(
                    $enum_name::$state(p) => p.session_id(),
                )+
            }
        }
    };

    // Delegate: device_id() -> DeviceId
    (@delegate_method device_id, $enum_name:ident, $( $state:ident ),+) => {
        pub fn device_id(&self) -> ::aura_journal::DeviceId {
            match self {
                $(
                    $enum_name::$state(p) => p.device_id(),
                )+
            }
        }
    };
}

/// Convenience macro to define an entire protocol in one block
///
/// This is a higher-level macro that combines all three previous macros
/// for maximum convenience when defining a new protocol.
///
/// # Syntax
///
/// ```rust,ignore
/// define_protocol! {
///     Protocol: ProtocolName,
///     Core: CoreType,
///     Error: ErrorType,
///     Union: UnionTypeName,
///
///     States {
///         State1 => OutputType1,
///         State2 => OutputType2,
///         @final State3 => OutputType3,
///     }
///
///     Extract {
///         session_id: |core| core.session_id,
///         device_id: |core| core.device_id,
///     }
/// }
/// ```
///
/// # Example
///
/// ```rust,ignore
/// define_protocol! {
///     Protocol: DkdProtocol,
///     Core: DkdProtocolCore,
///     Error: DkdSessionError,
///     Union: DkdProtocolState,
///
///     States {
///         InitializationPhase => (),
///         CommitmentPhase => [u8; 32],
///         RevealPhase => Vec<u8>,
///         FinalizationPhase => (),
///         @final CompletionPhase => Vec<u8>,
///         @final Failure => (),
///     }
///
///     Extract {
///         session_id: |core| core.session_id,
///         device_id: |core| core.device_id,
///     }
/// }
/// ```
#[macro_export]
macro_rules! define_protocol {
    (
        Protocol: $protocol:ident,
        Core: $core:ty,
        Error: $error:ty,
        Union: $union:ident,

        States {
            $( $state:ident $(@ $flag:ident )? => $output:ty ),+ $(,)?
        }

        Extract {
            session_id: |$core_var1:ident| $session_id_expr:expr,
            device_id: |$core_var2:ident| $device_id_expr:expr $(,)?
        }
    ) => {
        // Step 1: Define all session states
        define_session_states! {
            $( $state $(@ $flag )? ),+
        }

        // Step 2: Implement SessionProtocol for all states
        impl_session_protocol! {
            for $protocol<Core = $core, Error = $error> {
                $( $state => $output ),+
            }

            session_id: |$core_var1| $session_id_expr,
            device_id: |$core_var2| $device_id_expr,
        }

        // Step 3: Define union type with delegation
        define_session_union! {
            pub enum $union for $core {
                $( $state ),+
            }

            delegate: [state_name, can_terminate, protocol_id, device_id]
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test define_session_states! macro
    mod test_states {
        use crate::core::SessionState;

        define_session_states! {
            TestState1,
            TestState2,
            TestFinalState @ final,
        }

        #[test]
        fn test_non_final_state() {
            assert_eq!(TestState1::NAME, "TestState1");
            assert!(!TestState1::IS_FINAL);
            assert!(!TestState1::CAN_TERMINATE);

            assert_eq!(TestState2::NAME, "TestState2");
            assert!(!TestState2::IS_FINAL);
            assert!(!TestState2::CAN_TERMINATE);
        }

        #[test]
        fn test_final_state() {
            assert_eq!(TestFinalState::NAME, "TestFinalState");
            assert!(TestFinalState::IS_FINAL);
            assert!(TestFinalState::CAN_TERMINATE);
        }
    }

    // Test impl_session_protocol! macro
    mod test_protocol_impl {
        use crate::core::{ChoreographicProtocol, SessionProtocol, SessionState};
        use uuid::Uuid;

        #[derive(Debug, Clone)]
        pub struct TestCore {
            pub session_id: Uuid,
            pub device_id: aura_journal::DeviceId,
        }

        #[derive(Debug)]
        pub enum TestError {
            Failed,
        }

        define_session_states! {
            StateA,
            StateB @ final,
        }

        impl_session_protocol! {
            for TestProtocol<Core = TestCore, Error = TestError> {
                StateA => (),
                StateB => Vec<u8>,
            }

            session_id: |core| core.session_id,
            device_id: |core| core.device_id,
        }

        #[test]
        fn test_protocol_trait_impl() {
            let session_id = Uuid::new_v4();
            let device_id = aura_journal::DeviceId(Uuid::new_v4());
            let core = TestCore {
                session_id,
                device_id,
            };

            let protocol_a: ChoreographicProtocol<TestCore, StateA> =
                ChoreographicProtocol::new(core.clone());

            assert_eq!(protocol_a.session_id(), session_id);
            assert_eq!(protocol_a.state_name(), "StateA");
            assert!(!protocol_a.can_terminate());

            let protocol_b: ChoreographicProtocol<TestCore, StateB> =
                ChoreographicProtocol::new(core);

            assert_eq!(protocol_b.state_name(), "StateB");
            assert!(protocol_b.can_terminate());
        }
    }

    // Test define_session_union! macro
    mod test_union {
        use crate::core::{ChoreographicProtocol, SessionState};
        use uuid::Uuid;

        #[derive(Debug, Clone)]
        pub struct UnionTestCore {
            pub session_id: Uuid,
            pub device_id: aura_journal::DeviceId,
        }

        define_session_states! {
            UnionState1,
            UnionState2,
            UnionState3 @ final,
        }

        define_session_union! {
            pub enum TestUnion for UnionTestCore {
                UnionState1,
                UnionState2,
                UnionState3,
            }

            delegate: [state_name, can_terminate]
        }

        #[test]
        fn test_union_delegation() {
            let session_id = Uuid::new_v4();
            let device_id = aura_journal::DeviceId(Uuid::new_v4());
            let core = UnionTestCore {
                session_id,
                device_id,
            };

            let protocol1: ChoreographicProtocol<UnionTestCore, UnionState1> =
                ChoreographicProtocol::new(core.clone());
            let union1 = TestUnion::UnionState1(protocol1);

            assert_eq!(union1.state_name(), "UnionState1");
            assert!(!union1.can_terminate());

            let protocol3: ChoreographicProtocol<UnionTestCore, UnionState3> =
                ChoreographicProtocol::new(core);
            let union3 = TestUnion::UnionState3(protocol3);

            assert_eq!(union3.state_name(), "UnionState3");
            assert!(union3.can_terminate());
        }
    }

    // Test complete protocol definition macro
    mod test_complete_protocol {
        use uuid::Uuid;

        #[derive(Debug, Clone)]
        pub struct CompleteCore {
            pub session_id: Uuid,
            pub device_id: aura_journal::DeviceId,
        }

        #[derive(Debug)]
        pub enum CompleteError {
            Failed,
        }

        define_protocol! {
            Protocol: CompleteProtocol,
            Core: CompleteCore,
            Error: CompleteError,
            Union: CompleteProtocolState,

            States {
                Phase1 => (),
                Phase2 => String,
                Completed @ final => Vec<u8>,
                Failed @ final => (),
            }

            Extract {
                session_id: |core| core.session_id,
                device_id: |core| core.device_id,
            }
        }

        #[test]
        fn test_complete_protocol_definition() {
            use crate::core::{ChoreographicProtocol, SessionProtocol};

            let session_id = Uuid::new_v4();
            let device_id = aura_journal::DeviceId(Uuid::new_v4());
            let core = CompleteCore {
                session_id,
                device_id,
            };

            // Test that all states are defined
            let phase1: ChoreographicProtocol<CompleteCore, Phase1> =
                ChoreographicProtocol::new(core.clone());
            assert_eq!(phase1.state_name(), "Phase1");

            let phase2: ChoreographicProtocol<CompleteCore, Phase2> =
                ChoreographicProtocol::new(core.clone());
            assert_eq!(phase2.state_name(), "Phase2");

            let completed: ChoreographicProtocol<CompleteCore, Completed> =
                ChoreographicProtocol::new(core.clone());
            assert_eq!(completed.state_name(), "Completed");
            assert!(completed.can_terminate());

            // Test union type
            let union = CompleteProtocolState::Phase1(phase1);
            assert_eq!(union.state_name(), "Phase1");
            assert!(!union.can_terminate());
        }
    }
}
