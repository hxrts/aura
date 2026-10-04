use super::*;
use crate::runtime::RuntimeServiceLifecycleEvent;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

impl RuntimeSystem {
    /// Start runtime services using the RuntimeService trait.
    pub async fn start_services(&self) -> Result<(), ServiceError> {
        const STARTUP_RESOURCE_WINDOW: Duration = Duration::from_secs(30);
        let started_at = self.effect_system.physical_time().await.map_err(|source| {
            ServiceError::new(
                "runtime_startup",
                ServiceErrorKind::Unavailable,
                "required original startup observation failed",
            )
            .with_cause(source)
        })?;
        let window = TimeoutBudget::from_start_and_timeout(&started_at, STARTUP_RESOURCE_WINDOW)
            .map_err(|source| {
                ServiceError::new(
                    "runtime_startup",
                    ServiceErrorKind::InvalidConfiguration,
                    "invalid original startup resource window",
                )
                .with_cause(source)
            })?;
        let time: Arc<dyn PhysicalTimeEffects + Send + Sync> =
            Arc::new(self.effect_system.time_effects().clone());
        let context = RuntimeServiceContext::new(self.runtime_tasks.clone(), time, window.clone());
        let mut admitted_services = Vec::new();
        let started = execute_with_timeout_budget(self.effect_system.as_ref(), &window, || async {
            self.authority_manager
                .ensure_authority(self.authority_id, started_at.ts_ms)
                .await
                .map_err(|source| {
                    ServiceError::startup_failed("authority_manager", "prepare original authority")
                        .with_cause(source)
                })?;
            self.authority_manager
                .set_status(self.authority_id, AuthorityStatus::Active, started_at.ts_ms)
                .await
                .map_err(|source| {
                    ServiceError::startup_failed(
                        "authority_manager",
                        "publish active authority status",
                    )
                    .with_cause(source)
                })?;
            for service in self.runtime_services_in_start_order()? {
                // Include partially started service in required cleanup custody.
                admitted_services.push(service);
                self.start_runtime_service(service, &context).await?;
            }
            Ok::<(), ServiceError>(())
        })
        .await
        .map_err(|source| {
            crate::runtime::services::traits::service_window_failure("runtime_startup", source)
        });
        if let Err(primary) = started {
            let mut cleanup = Vec::new();
            for service in admitted_services.into_iter().rev() {
                if let Err(source) = self.cleanup_partially_started_service(service).await {
                    cleanup.push(source);
                }
            }
            if cleanup.is_empty() {
                return Err(primary);
            }
            let kind = primary.kind.clone();
            return Err(ServiceError::new(
                "runtime_startup",
                kind,
                "runtime startup failed and partial-start cleanup also failed",
            )
            .with_cause(RuntimeStartupCleanupFailure { primary, cleanup }));
        }
        // Optional initial descriptor publication cannot delay primary readiness.
        // It remains in the same actual supervisor and original startup window.
        let maintenance = self.maintenance_service.clone();
        let effects = self.effect_system.clone();
        let tasks = context.tasks().group("runtime_startup_maintenance");
        let attempt = async move {
            execute_with_timeout_budget(effects.as_ref(), &window, || {
                maintenance.publish_initial_lan_descriptor()
            })
            .await
            .map_err(|source| aura_core::AuraError::Internal {
                message: "subsidiary initial LAN descriptor publication failed".into(),
                source: Some(Arc::new(
                    crate::runtime::services::traits::service_window_failure(
                        "runtime_startup_maintenance",
                        source,
                    ),
                )),
            })
        };
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _owned_attempt = tasks.spawn_local_try_named("initial_lan_descriptor", attempt);
            } else {
                let _owned_attempt = tasks.spawn_try_named("initial_lan_descriptor", attempt);
            }
        }
        Ok(())
    }

    #[aura_macros::capability_boundary(category = "capability_gated",
        capability = "RuntimeShutdownWindowCapability", capability_type = RuntimeShutdownWindowCapability,
        family = "runtime_helper")]
    pub(in crate::runtime) async fn stop_services(
        &self,
        original: &RuntimeShutdownWindowCapability,
    ) -> Result<(), ServiceError> {
        original.require_runtime(self).map_err(|source| {
            ServiceError::shutdown_failed("runtime_services", "foreign original shutdown owner")
                .with_cause(source)
        })?;
        for service in self.runtime_services_in_stop_order()? {
            self.stop_runtime_service(service, original).await?;
        }

        Ok(())
    }

    fn runtime_services(&self) -> Vec<&dyn RuntimeService> {
        let mut services: Vec<&dyn RuntimeService> = vec![
            &self.reactive_pipeline_service,
            &self.flow_budget_manager,
            &self.receipt_manager,
            &self.ceremony_tracker,
            &self.threshold_signing,
        ];
        if let Some(social_manager) = &self.social_manager {
            services.push(social_manager);
        }
        if let Some(rendezvous_manager) = &self.rendezvous_manager {
            services.push(rendezvous_manager);
        }
        if let Some(move_manager) = &self.move_manager {
            services.push(move_manager);
        }
        if let Some(local_health_observer) = &self.local_health_observer {
            services.push(local_health_observer);
        }
        if let Some(selection_manager) = &self.selection_manager {
            services.push(selection_manager);
        }
        if let Some(anonymous_path_manager) = &self.anonymous_path_manager {
            services.push(anonymous_path_manager);
        }
        if let Some(hold_manager) = &self.hold_manager {
            services.push(hold_manager);
        }
        if let Some(cover_traffic_generator) = &self.cover_traffic_generator {
            services.push(cover_traffic_generator);
        }
        if let Some(sync_manager) = &self.sync_manager {
            services.push(sync_manager);
        }
        if let Some(lan_listener_service) = &self.lan_listener_service {
            services.push(lan_listener_service);
        }
        services.push(&self.sync_command_registry);
        services.push(&self.maintenance_service);
        services
    }

    pub(crate) fn runtime_services_in_start_order(
        &self,
    ) -> Result<Vec<&dyn RuntimeService>, ServiceError> {
        sort_runtime_services_by_dependencies(self.runtime_services())
    }

    fn runtime_services_in_stop_order(&self) -> Result<Vec<&dyn RuntimeService>, ServiceError> {
        let mut services = self.runtime_services_in_start_order()?;
        services.reverse();
        Ok(services)
    }

    async fn start_runtime_service(
        &self,
        service: &dyn RuntimeService,
        context: &RuntimeServiceContext,
    ) -> Result<(), ServiceError> {
        tracing::info!(
            event = RuntimeServiceLifecycleEvent::Transition.as_event_name(),
            service = service.name(),
            phase = "start_requested",
            "Starting runtime service"
        );
        let health = execute_with_timeout_budget(
            self.effect_system.as_ref(),
            context.startup_window(),
            || async {
                service.start(context).await?;
                Ok::<_, ServiceError>(service.health().await)
            },
        )
        .await
        .map_err(|source| {
            crate::runtime::services::traits::service_window_failure(service.name(), source)
        })?;
        match health {
            ServiceHealth::Healthy | ServiceHealth::Degraded { .. } => {
                tracing::info!(
                    event = RuntimeServiceLifecycleEvent::Transition.as_event_name(),
                    service = service.name(),
                    phase = "running",
                    health = %health,
                    "Runtime service started"
                );
                Ok(())
            }
            other => Err(ServiceError::startup_failed(
                service.name(),
                format!("service entered non-operational state after start: {other}"),
            )),
        }
    }

    #[aura_macros::capability_boundary(category = "capability_gated",
        capability = "RuntimeShutdownWindowCapability", capability_type = RuntimeShutdownWindowCapability,
        family = "runtime_helper")]
    pub(in crate::runtime) async fn stop_runtime_service(
        &self,
        service: &dyn RuntimeService,
        original: &RuntimeShutdownWindowCapability,
    ) -> Result<(), ServiceError> {
        original.require_runtime(self).map_err(|source| {
            ServiceError::shutdown_failed(service.name(), "foreign original shutdown owner")
                .with_cause(source)
        })?;
        // Stop and its required health observation share the same original clock.
        // No per-service five-second budget is born during this continuation.
        let health =
            execute_with_timeout_budget(self.effect_system.as_ref(), original.budget(), || async {
                service.stop().await?;
                Ok::<_, ServiceError>(service.health().await)
            })
            .await
            .map_err(|source| match source {
                TimeoutRunError::Operation(source) => source,
                TimeoutRunError::Timeout(
                    source @ aura_core::TimeoutBudgetError::DeadlineExceeded { .. },
                ) => {
                    tracing::warn!(
                        event = RuntimeShutdownEvent::ServiceTimeout.as_event_name(),
                        service = service.name(),
                        "Original shutdown window expired during required service disposal"
                    );
                    ServiceError::new(
                        service.name(),
                        ServiceErrorKind::Timeout,
                        "original shutdown resource deadline exceeded",
                    )
                    .with_cause(source)
                }
                TimeoutRunError::Timeout(source) => ServiceError::new(
                    service.name(),
                    ServiceErrorKind::Internal,
                    "required original shutdown resource observation failed",
                )
                .with_cause(source),
            })?;
        match health {
            ServiceHealth::Stopped | ServiceHealth::NotStarted => Ok(()),
            other => Err(ServiceError::shutdown_failed(
                service.name(),
                format!("service remained active after required stop: {other}"),
            )),
        }
    }

    async fn cleanup_partially_started_service(
        &self,
        service: &dyn RuntimeService,
    ) -> Result<(), ServiceError> {
        const SERVICE_STOP_TIMEOUT: Duration = Duration::from_secs(5);

        tracing::info!(
            event = RuntimeServiceLifecycleEvent::Transition.as_event_name(),
            service = service.name(),
            phase = "stop_requested",
            "Stopping runtime service"
        );
        let started_at = self.effect_system.physical_time().await.map_err(|source| {
            ServiceError::unavailable(service.name(), "required cleanup clock failed")
                .with_cause(source)
        })?;
        let budget = TimeoutBudget::from_start_and_timeout(&started_at, SERVICE_STOP_TIMEOUT)
            .map_err(|source| {
                ServiceError::new(
                    service.name(),
                    ServiceErrorKind::InvalidConfiguration,
                    "invalid original cleanup window",
                )
                .with_cause(source)
            })?;
        // Stop and its required health observation share one cleanup window;
        // neither successful stop nor timer failure grants indefinite health wait.
        let health = execute_with_timeout_budget(self.effect_system.as_ref(), &budget, || async {
            service.stop().await?;
            Ok(service.health().await)
        })
        .await
        .map_err(|source| {
            crate::runtime::services::traits::service_window_failure(service.name(), source)
        })?;
        match health {
            ServiceHealth::Stopped | ServiceHealth::NotStarted => {
                tracing::info!(
                    event = RuntimeServiceLifecycleEvent::Transition.as_event_name(),
                    service = service.name(),
                    phase = "stopped",
                    health = %health,
                    "Runtime service stopped"
                );
                Ok(())
            }
            other => Err(ServiceError::shutdown_failed(
                service.name(),
                format!("service remained active after stop: {other}"),
            )),
        }
    }
}

fn sort_runtime_services_by_dependencies(
    services: Vec<&dyn RuntimeService>,
) -> Result<Vec<&dyn RuntimeService>, ServiceError> {
    let mut service_by_name = BTreeMap::new();
    for service in &services {
        service_by_name.insert(service.name(), *service);
    }

    let mut indegree = BTreeMap::<&'static str, usize>::new();
    let mut dependents = BTreeMap::<&'static str, Vec<&'static str>>::new();
    for service in &services {
        indegree.entry(service.name()).or_insert(0);
        for dependency in service.dependencies() {
            if !service_by_name.contains_key(dependency) {
                continue;
            }
            *indegree.entry(service.name()).or_insert(0) += 1;
            dependents
                .entry(*dependency)
                .or_default()
                .push(service.name());
        }
    }

    let mut ready = VecDeque::new();
    for service in &services {
        if indegree.get(service.name()).copied().unwrap_or_default() == 0 {
            ready.push_back(service.name());
        }
    }

    let mut ordered = Vec::with_capacity(services.len());
    while let Some(name) = ready.pop_front() {
        let Some(service) = service_by_name.get(name).copied() else {
            continue;
        };
        ordered.push(service);
        if let Some(children) = dependents.get(name) {
            for child in children {
                if let Some(entry) = indegree.get_mut(child) {
                    *entry = entry.saturating_sub(1);
                    if *entry == 0 {
                        ready.push_back(child);
                    }
                }
            }
        }
    }

    if ordered.len() != services.len() {
        let blocked = indegree
            .into_iter()
            .filter_map(|(name, count)| (count > 0).then_some(name))
            .collect::<Vec<_>>();
        return Err(ServiceError::new(
            "runtime_services",
            ServiceErrorKind::DependencyUnavailable,
            format!(
                "runtime service dependency graph contains a cycle or unsatisfied internal dependencies: {}",
                blocked.join(", ")
            ),
        ));
    }

    Ok(ordered)
}

#[derive(Debug, thiserror::Error)]
#[error("startup failed and required cleanup of {count} partially started service(s) failed", count = .cleanup.len())]
struct RuntimeStartupCleanupFailure {
    #[source]
    primary: ServiceError,
    cleanup: Vec<ServiceError>,
}
impl RuntimeStartupCleanupFailure {
    #[cfg(test)]
    fn cleanup_causes(&self) -> &[ServiceError] {
        &self.cleanup
    }
}

#[cfg(test)]
mod startup_source_tests {
    use super::*;
    use aura_core::effects::time::TimeError;
    use aura_testkit::time::ManualPhysicalClock;
    use std::error::Error;

    fn source_of<'a, T: Error + 'static>(error: &'a (dyn Error + 'static)) -> Option<&'a T> {
        let mut current = Some(error);
        while let Some(cause) = current {
            if let Some(native) = cause.downcast_ref::<T>() {
                return Some(native);
            }
            current = cause.source();
        }
        None
    }

    #[tokio::test]
    async fn required_startup_cleanup_aggregate_retains_actual_primary_and_secondary_sources() {
        let clock = ManualPhysicalClock::new(1000);
        clock
            .fail_next_observation(TimeError::OperationFailed {
                reason: "actual startup read fault".into(),
            })
            .await;
        let primary = clock
            .physical_time()
            .await
            .expect_err("actual configured clock read failure");
        clock
            .fail_next_sleep(TimeError::OperationFailed {
                reason: "actual cleanup sleep fault".into(),
            })
            .await;
        let secondary = clock
            .sleep_ms(1)
            .await
            .expect_err("actual configured cleanup sleep failure");
        let failure = RuntimeStartupCleanupFailure {
            primary: ServiceError::startup_failed("actual_startup", "required startup failed")
                .with_cause(primary),
            cleanup: vec![ServiceError::shutdown_failed(
                "actual_cleanup",
                "required cleanup failed",
            )
            .with_cause(secondary)],
        };
        assert!(
            matches!(source_of::<TimeError>(&failure), Some(TimeError::OperationFailed { reason })
            if reason == "actual startup read fault")
        );
        assert!(
            matches!(source_of::<TimeError>(&failure.cleanup_causes()[0]), Some(TimeError::OperationFailed { reason })
            if reason == "actual cleanup sleep fault")
        );
        assert_eq!(failure.cleanup_causes().len(), 1);
    }
}
