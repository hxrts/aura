use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::effects::{PhysicalTimeEffects, RandomCoreEffects};
use aura_core::DeviceId;
use aura_protocol::effects::{EffectApiError, EffectApiEventStream};
use futures::channel::mpsc;
use std::collections::{HashMap, HashSet, VecDeque};

fn build_directed_graph(edges: &[(Vec<u8>, Vec<u8>)]) -> HashMap<Vec<u8>, Vec<Vec<u8>>> {
    let mut graph = HashMap::<Vec<u8>, Vec<Vec<u8>>>::new();
    for (from, to) in edges {
        graph.entry(from.clone()).or_default().push(to.clone());
        graph.entry(to.clone()).or_default();
    }
    graph
}

fn build_reverse_graph(graph: &HashMap<Vec<u8>, Vec<Vec<u8>>>) -> HashMap<Vec<u8>, Vec<Vec<u8>>> {
    let mut reverse = HashMap::<Vec<u8>, Vec<Vec<u8>>>::new();
    for (node, neighbors) in graph {
        reverse.entry(node.clone()).or_default();
        for neighbor in neighbors {
            reverse
                .entry(neighbor.clone())
                .or_default()
                .push(node.clone());
        }
    }
    reverse
}

fn graph_has_path(graph: &HashMap<Vec<u8>, Vec<Vec<u8>>>, start: &[u8], target: &[u8]) -> bool {
    let mut queue = VecDeque::from([start.to_vec()]);
    let mut seen = HashSet::<Vec<u8>>::new();
    while let Some(node) = queue.pop_front() {
        if node == target {
            return true;
        }
        if !seen.insert(node.clone()) {
            continue;
        }
        if let Some(neighbors) = graph.get(&node) {
            for neighbor in neighbors {
                queue.push_back(neighbor.clone());
            }
        }
    }
    false
}

fn dfs_finish_order(
    graph: &HashMap<Vec<u8>, Vec<Vec<u8>>>,
    node: Vec<u8>,
    seen: &mut HashSet<Vec<u8>>,
    order: &mut Vec<Vec<u8>>,
) {
    if !seen.insert(node.clone()) {
        return;
    }
    if let Some(neighbors) = graph.get(&node) {
        for neighbor in neighbors {
            dfs_finish_order(graph, neighbor.clone(), seen, order);
        }
    }
    order.push(node);
}

fn dfs_collect_component(
    graph: &HashMap<Vec<u8>, Vec<Vec<u8>>>,
    node: Vec<u8>,
    seen: &mut HashSet<Vec<u8>>,
    component: &mut Vec<Vec<u8>>,
) {
    if !seen.insert(node.clone()) {
        return;
    }
    component.push(node.clone());
    if let Some(neighbors) = graph.get(&node) {
        for neighbor in neighbors {
            dfs_collect_component(graph, neighbor.clone(), seen, component);
        }
    }
}

// Implementation of EffectApiEffects
#[async_trait]
impl aura_protocol::effects::EffectApiEffects for AuraEffectSystem {
    async fn append_event(&self, event: Vec<u8>) -> Result<(), EffectApiError> {
        let epoch = {
            let mut ledger = self.effect_api_ledger.lock();
            ledger.epoch = ledger.epoch.saturating_add(1);
            ledger.epoch
        };
        self.effect_api_append(event, epoch);
        Ok(())
    }

    async fn current_epoch(&self) -> Result<u64, EffectApiError> {
        Ok(self.effect_api_ledger.lock().epoch)
    }

    async fn events_since(&self, epoch: u64) -> Result<Vec<Vec<u8>>, EffectApiError> {
        let ledger = self.effect_api_ledger.lock();
        if epoch > ledger.epoch {
            return Err(EffectApiError::EpochOutOfRange { epoch });
        }
        Ok(ledger
            .events
            .iter()
            .filter(|(event_epoch, _)| *event_epoch > epoch)
            .map(|(_, event)| event.clone())
            .collect())
    }

    async fn is_device_authorized(
        &self,
        device_id: DeviceId,
        _operation: &str,
    ) -> Result<bool, EffectApiError> {
        Ok(device_id == self.device_id() || self.biscuit_cache.read().is_some())
    }

    async fn update_device_activity(&self, device_id: DeviceId) -> Result<(), EffectApiError> {
        let last_seen = self.current_timestamp().await?;
        self.effect_api_ledger
            .lock()
            .device_activity
            .insert(device_id, last_seen);
        self.effect_api_publish_device_activity(device_id, last_seen);
        Ok(())
    }

    async fn subscribe_to_events(&self) -> Result<EffectApiEventStream, EffectApiError> {
        let (sender, receiver) = mpsc::channel(64);
        self.effect_api_ledger.lock().subscribers.push(sender);
        Ok(Box::new(Box::pin(receiver)))
    }

    async fn would_create_cycle(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
        new_edge: (Vec<u8>, Vec<u8>),
    ) -> Result<bool, EffectApiError> {
        let mut graph = build_directed_graph(edges);
        graph
            .entry(new_edge.0.clone())
            .or_default()
            .push(new_edge.1.clone());
        graph.entry(new_edge.1.clone()).or_default();
        Ok(graph_has_path(&graph, &new_edge.1, &new_edge.0))
    }

    async fn find_connected_components(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<Vec<u8>>>, EffectApiError> {
        let graph = build_directed_graph(edges);
        let reverse = build_reverse_graph(&graph);
        let mut seen = HashSet::<Vec<u8>>::new();
        let mut order = Vec::new();
        for node in graph.keys() {
            dfs_finish_order(&graph, node.clone(), &mut seen, &mut order);
        }
        let mut reverse_seen = HashSet::<Vec<u8>>::new();
        let mut components = Vec::new();
        for node in order.into_iter().rev() {
            if reverse_seen.contains(&node) {
                continue;
            }
            let mut component = Vec::new();
            dfs_collect_component(&reverse, node, &mut reverse_seen, &mut component);
            if !component.is_empty() {
                components.push(component);
            }
        }
        Ok(components)
    }

    async fn topological_sort(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<u8>>, EffectApiError> {
        let graph = build_directed_graph(edges);
        let mut indegree = HashMap::<Vec<u8>, usize>::new();
        for node in graph.keys() {
            indegree.entry(node.clone()).or_insert(0);
        }
        for neighbors in graph.values() {
            for neighbor in neighbors {
                *indegree.entry(neighbor.clone()).or_insert(0) += 1;
            }
        }
        let mut queue = indegree
            .iter()
            .filter_map(|(node, degree)| (*degree == 0).then(|| node.clone()))
            .collect::<VecDeque<_>>();
        let mut ordered = Vec::new();
        let mut remaining = indegree;
        while let Some(node) = queue.pop_front() {
            ordered.push(node.clone());
            if let Some(neighbors) = graph.get(&node) {
                for neighbor in neighbors {
                    if let Some(entry) = remaining.get_mut(neighbor) {
                        *entry = entry.saturating_sub(1);
                        if *entry == 0 {
                            queue.push_back(neighbor.clone());
                        }
                    }
                }
            }
        }
        if ordered.len() != remaining.len() {
            return Err(EffectApiError::GraphOperationFailed {
                message: "topological sort failed because the graph contains a cycle".to_string(),
            });
        }
        Ok(ordered)
    }

    async fn shortest_path(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
        start: Vec<u8>,
        end: Vec<u8>,
    ) -> Result<Option<Vec<Vec<u8>>>, EffectApiError> {
        let graph = build_directed_graph(edges);
        let mut queue = VecDeque::from([start.clone()]);
        let mut seen = HashSet::<Vec<u8>>::new();
        let mut parent = HashMap::<Vec<u8>, Vec<u8>>::new();
        while let Some(node) = queue.pop_front() {
            if !seen.insert(node.clone()) {
                continue;
            }
            if node == end {
                let mut path = vec![end.clone()];
                let mut cursor = end;
                while let Some(prev) = parent.get(&cursor) {
                    path.push(prev.clone());
                    cursor = prev.clone();
                }
                path.reverse();
                return Ok(Some(path));
            }
            if let Some(neighbors) = graph.get(&node) {
                for neighbor in neighbors {
                    if !seen.contains(neighbor) {
                        parent
                            .entry(neighbor.clone())
                            .or_insert_with(|| node.clone());
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }
        Ok(None)
    }

    async fn generate_secret(&self, length: usize) -> Result<Vec<u8>, EffectApiError> {
        Ok(self.random_bytes(length).await)
    }

    async fn hash_data(&self, data: &[u8]) -> Result<[u8; 32], EffectApiError> {
        // Mock implementation - simple hash
        use aura_core::hash::hash;
        Ok(hash(data))
    }

    async fn current_timestamp(&self) -> Result<u64, EffectApiError> {
        // Use PhysicalTimeEffects instead of direct SystemTime
        let physical_time =
            self.time_handler
                .physical_time()
                .await
                .map_err(|e| EffectApiError::Backend {
                    error: format!("time unavailable: {e}"),
                })?;
        Ok(physical_time.ts_ms / 1000)
    }

    async fn effect_api_device_id(&self) -> Result<DeviceId, EffectApiError> {
        Ok(self.device_id())
    }

    async fn new_uuid(&self) -> Result<uuid::Uuid, EffectApiError> {
        let mut bytes: [u8; 16] = self.random_bytes(16).await.try_into().map_err(|_| {
            EffectApiError::CryptoOperationFailed {
                message: "failed to generate random UUID bytes".to_string(),
            }
        })?;
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Ok(uuid::Uuid::from_bytes(bytes))
    }
}
