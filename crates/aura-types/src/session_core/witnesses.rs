//! Runtime Witnesses for Distributed Invariants
//!
//! This module provides runtime witness types that can only be constructed after
//! verifying distributed protocol conditions through journal evidence. These witnesses
//! enable session type transitions that depend on distributed state.

/// Trait for types that can serve as runtime witnesses
///
/// Runtime witnesses are proof objects that can only be constructed after verifying
/// that a distributed condition has been met through examination of journal evidence.
pub trait RuntimeWitness: 'static + Send + Sync {
    /// The type of evidence required to construct this witness
    type Evidence;

    /// The type of configuration or parameters needed for verification
    type Config;

    /// Attempt to construct the witness from evidence
    ///
    /// Returns `Some(witness)` if the distributed condition is satisfied,
    /// `None` otherwise.
    fn verify(evidence: Self::Evidence, config: Self::Config) -> Option<Self>
    where
        Self: Sized;

    /// Get a description of what this witness proves (for debugging)
    fn description(&self) -> &'static str;
}

/// Universal RuntimeWitness implementation for unit type (used for simple transitions)
impl RuntimeWitness for () {
    type Evidence = ();
    type Config = ();

    fn verify(_evidence: (), _config: ()) -> Option<Self> {
        Some(())
    }

    fn description(&self) -> &'static str {
        "No witness required"
    }
}

/// RuntimeWitness implementation for String
impl RuntimeWitness for String {
    type Evidence = String;
    type Config = ();

    fn verify(evidence: String, _config: ()) -> Option<Self> {
        Some(evidence)
    }

    fn description(&self) -> &'static str {
        "String value provided"
    }
}

/// RuntimeWitness implementation for bool
impl RuntimeWitness for bool {
    type Evidence = bool;
    type Config = ();

    fn verify(evidence: bool, _config: ()) -> Option<Self> {
        Some(evidence)
    }

    fn description(&self) -> &'static str {
        "Boolean condition verified"
    }
}

/// RuntimeWitness implementation for `Vec<T>`
impl<T: Send + Sync + 'static> RuntimeWitness for Vec<T> {
    type Evidence = Vec<T>;
    type Config = ();

    fn verify(evidence: Vec<T>, _config: ()) -> Option<Self> {
        Some(evidence)
    }

    fn description(&self) -> &'static str {
        "Vector provided"
    }
}

/// RuntimeWitness implementation for `Option<T>`
impl<T: RuntimeWitness> RuntimeWitness for Option<T> {
    type Evidence = Option<T::Evidence>;
    type Config = T::Config;

    fn verify(evidence: Option<T::Evidence>, config: T::Config) -> Option<Self> {
        match evidence {
            Some(inner_evidence) => T::verify(inner_evidence, config).map(Some),
            None => Some(None),
        }
    }

    fn description(&self) -> &'static str {
        "Optional witness"
    }
}

/// RuntimeWitness implementation for `(T, U)` tuple
impl<T: RuntimeWitness, U: RuntimeWitness> RuntimeWitness for (T, U) {
    type Evidence = (T::Evidence, U::Evidence);
    type Config = (T::Config, U::Config);

    fn verify(
        evidence: (T::Evidence, U::Evidence),
        config: (T::Config, U::Config),
    ) -> Option<Self> {
        let (t_evidence, u_evidence) = evidence;
        let (t_config, u_config) = config;

        let t_witness = T::verify(t_evidence, t_config)?;
        let u_witness = U::verify(u_evidence, u_config)?;

        Some((t_witness, u_witness))
    }

    fn description(&self) -> &'static str {
        "Tuple witness"
    }
}

/// Simple witness proving an event count threshold has been met
#[derive(Debug, Clone)]
pub struct ThresholdMet {
    /// Current count of events
    pub count: usize,
    /// Required threshold that must be met
    pub threshold: usize,
}

impl RuntimeWitness for ThresholdMet {
    type Evidence = usize; // event count
    type Config = usize; // threshold

    fn verify(count: usize, threshold: usize) -> Option<Self> {
        if count >= threshold {
            Some(ThresholdMet { count, threshold })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Event threshold met"
    }
}
