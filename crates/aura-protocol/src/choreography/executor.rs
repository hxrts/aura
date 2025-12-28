//! MPST executor for running generated local session types.
//!
//! This executor bridges `LocalSessionType` values to concrete protocol runners
//! registered by the application or feature crates.

use std::collections::HashMap;
use std::future::Future;

use aura_mpst::session::LocalSessionType;
use futures::future::BoxFuture;
use rumpsteak_aura_choreography::ChoreographyError;

use super::{ChoreographicAdapter, ChoreographicEndpoint};

type SessionRunner<A> = Box<
    dyn for<'a> Fn(
            &'a mut A,
            &'a mut <A as ChoreographicAdapter>::Endpoint,
            &'a [u8],
        ) -> BoxFuture<'a, ChoreoResult>
        + Send
        + Sync,
>;

type ChoreoResult = Result<(), ChoreographyError>;

/// Executor that dispatches local session types to registered protocol runners.
pub struct MpstExecutor<A: ChoreographicAdapter> {
    adapter: A,
    runners: HashMap<String, SessionRunner<A>>,
}

impl<A: ChoreographicAdapter> MpstExecutor<A> {
    /// Create a new executor for a given adapter.
    pub fn new(adapter: A) -> Self {
        Self {
            adapter,
            runners: HashMap::new(),
        }
    }

    /// Register a runner for a protocol name.
    pub fn register_runner<F, Fut>(&mut self, protocol: impl Into<String>, runner: F)
    where
        F: for<'a> Fn(
                &'a mut A,
                &'a mut <A as ChoreographicAdapter>::Endpoint,
                &'a [u8],
            ) -> Fut
            + Send
            + Sync
            + 'static,
        Fut: Future<Output = ChoreoResult> + Send + 'static,
    {
        let wrapped: SessionRunner<A> = Box::new(move |adapter, endpoint, params| {
            let fut = runner(adapter, endpoint, params);
            Box::pin(fut)
        });
        self.runners.insert(protocol.into(), wrapped);
    }

    /// Execute a local session type using a registered runner.
    pub async fn execute(&mut self, session: &LocalSessionType) -> ChoreoResult {
        let runner = self.runners.get(session.protocol()).ok_or_else(|| {
            ChoreographyError::Transport(format!(
                "No registered runner for protocol {}",
                session.protocol()
            ))
        })?;

        let mut endpoint = <A as ChoreographicAdapter>::Endpoint::new(self.adapter.device_id());
        runner(&mut self.adapter, &mut endpoint, session.params()).await
    }

    /// Access the underlying adapter for configuration.
    pub fn adapter_mut(&mut self) -> &mut A {
        &mut self.adapter
    }
}
