//! Handler isolation and roundtrip integration tests.

mod handlers {
    mod encrypted_storage_roundtrip;
    mod guard_interpreter;
    mod impure_api_confinement;
}
