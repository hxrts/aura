use super::*;
use crate::runtime::RuntimeServiceLifecycleEvent;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

impl RuntimeSystem {
    /// Start runtime services using the RuntimeService trait.
    pub async fn start_services(&self) -> Result<(), ServiceError> {
        let now_ms = self
            .effect_system
            .time_effects()
            .physical_time()
            .await
            .map_err(|e| ServiceError::startup_failed("authority_manager", e.to_string()))?
            .ts_ms;
        self.authority_manager
            .ensure_authority(self.authority_id, now_ms)
            .await
            .map_err(|e| ServiceError::startup_failed("authority_manager", e.to_string()))?;
        self.authority_manager
            .set_status(self.authority_id, AuthorityStatus::Active, now_ms)
            .await
            .map_err(|e| ServiceError::startup_failed("authority_manager", e.to_string()))?;

        let time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync> =
            Arc::new(self.effect_system.time_effects().clone());
        let service_context = RuntimeServiceContext::new(self.runtime_tasks.clone(), time_effects);

        for service in self.runtime_services_in_start_order()? {
            self.start_runtime_service(service, &service_context)
                .await?;
        }

        if let Err(error) = self
            .maintenance_service
            .publish_initial_lan_descriptor()
            .await
        {
            tracing::warn!(
                event = RuntimeServiceLifecycleEvent::ReconcileFailed.as_event_name(),
                service = self.maintenance_service.name(),
                error = %error,
                "Runtime startup reconciliation failed to republish the initial LAN descriptor"
            );
        }

        Ok(())
    }

    pub(super) async fn stop_services(&self) -> Result<(), ServiceError> {
        let now_ms = self
            .effect_system
            .time_effects()
            .physical_time()
            .await
            .map_err(|e| ServiceError::shutdown_failed("authority_manager", e.to_string()))?
            .ts_ms;
        self.authority_manager
            .set_status(self.authority_id, AuthorityStatus::Terminated, now_ms)
            .await
            .map_err(|e| ServiceError::shutdown_failed("authority_manager", e.to_string()))?;

        for service in self.runtime_services_in_stop_order()? {
            self.stop_runtime_service(service).await?;
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
        service.start(context).await?;
        let health = service.health().await;
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

    async fn stop_runtime_service(&self, service: &dyn RuntimeService) -> Result<(), ServiceError> {
        const SERVICE_STOP_TIMEOUT: Duration = Duration::from_secs(5);

        tracing::info!(
            event = RuntimeServiceLifecycleEvent::Transition.as_event_name(),
            service = service.name(),
            phase = "stop_requested",
            "Stopping runtime service"
        );
        let started_at = self.effect_system.physical_time().await.map_err(|error| {
            ServiceError::new(
                service.name(),
                ServiceErrorKind::Internal,
                format!("could not read physical time for service stop budget: {error}"),
            )
        })?;
        let budget = TimeoutBudget::from_start_and_timeout(&started_at, SERVICE_STOP_TIMEOUT)
            .map_err(|error| {
                ServiceError::new(
                    service.name(),
                    ServiceErrorKind::Internal,
                    format!("invalid service stop timeout budget: {error}"),
                )
            })?;

        execute_with_timeout_budget(self.effect_system.as_ref(), &budget, || service.stop())
            .await
            .map_err(|error| match error {
                TimeoutRunError::Timeout(_) => {
                    tracing::warn!(
                        event = RuntimeShutdownEvent::ServiceTimeout.as_event_name(),
                        service = service.name(),
                        timeout_ms = SERVICE_STOP_TIMEOUT.as_millis() as u64,
                        "Runtime service stop timed out"
                    );
                    ServiceError::new(
                        service.name(),
                        ServiceErrorKind::Timeout,
                        format!(
                            "service stop timed out after {}ms",
                            SERVICE_STOP_TIMEOUT.as_millis()
                        ),
                    )
                }
                TimeoutRunError::Operation(error) => ServiceError::new(
                    service.name(),
                    ServiceErrorKind::Internal,
                    error.to_string(),
                ),
            })?;
        let health = service.health().await;
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
