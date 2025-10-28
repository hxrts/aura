//! Declarative macros for reducing session type boilerplate
//!
//! This module provides macros to dramatically reduce the verbosity of defining
//! session-typed protocols. These macros generate the repetitive trait implementations
//! while maintaining full compile-time type safety.

/// Define multiple session states with minimal boilerplate
///
/// This macro generates:
/// - A struct for each state with `#[derive(Debug, Clone)]`
/// - A `SessionState` trait implementation with correct `NAME`
/// - Correct `IS_FINAL` and `CAN_TERMINATE` flags for terminal states
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

                fn device_id(&self) -> ::uuid::Uuid {
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

        pub fn device_id(&self) -> ::uuid::Uuid {
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
}

/// Convenience macro to define an entire protocol in one block
///
/// This is a higher-level macro that combines all three previous macros
/// for maximum convenience when defining a new protocol.
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
