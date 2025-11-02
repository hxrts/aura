//! Integration tests for choreographic protocols with middleware stack

#[cfg(test)]
mod tests {
    use aura_crypto::Effects;
    use aura_protocol::{
        effects::AuraEffectsAdapter,
        execution::context::BaseContext,
        handlers::InMemoryHandler,
        middleware::EffectsMiddleware,
        protocols::{
            BridgedEndpoint, BridgedRole, DecentralizedLottery, DkdProtocol, FrostSigningProtocol,
            RumpsteakAdapter,
        },
    };
    use uuid::Uuid;

    #[tokio::test]
    async fn test_lottery_with_middleware() {
        let effects = Effects::test(42);
        let device_ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();

        // Create participants
        let participants: Vec<BridgedRole> = device_ids
            .iter()
            .enumerate()
            .map(|(i, &id)| BridgedRole {
                device_id: id,
                role_index: i,
            })
            .collect();

        // Set up handler with middleware
        let handler = EffectsMiddleware::new(InMemoryHandler::new(), effects.clone());

        let context = BaseContext::new(device_ids[0], effects.clone(), None, None);

        let mut adapter = RumpsteakAdapter::new(
            handler,
            AuraEffectsAdapter::new(device_ids[0], effects),
            context,
        );

        let mut endpoint = BridgedEndpoint {
            context: BaseContext::new(device_ids[0], effects.clone(), None, None),
        };

        // Run lottery
        let lottery = DecentralizedLottery::new(participants.clone());
        let result = lottery
            .execute(&mut adapter, &mut endpoint, participants[0])
            .await;

        assert!(result.is_ok());
        let selected = result.unwrap();
        assert!(participants.contains(&selected));
    }

    #[tokio::test]
    async fn test_dkd_with_middleware() {
        let effects = Effects::test(42);
        let device_ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();

        let participants: Vec<BridgedRole> = device_ids
            .iter()
            .enumerate()
            .map(|(i, &id)| BridgedRole {
                device_id: id,
                role_index: i,
            })
            .collect();

        let handler = EffectsMiddleware::new(InMemoryHandler::new(), effects.clone());

        let context = BaseContext::new(device_ids[0], effects.clone(), None, None);

        let mut adapter = RumpsteakAdapter::new(
            handler,
            AuraEffectsAdapter::new(device_ids[0], effects.clone()),
            context,
        );

        let mut endpoint = BridgedEndpoint {
            context: BaseContext::new(device_ids[0], effects.clone(), None, None),
        };

        // Run DKD
        let dkd = DkdProtocol::new(
            participants.clone(),
            "test-app".to_string(),
            "test-context".to_string(),
        );

        let result = dkd
            .execute(&mut adapter, &mut endpoint, participants[0])
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 32);
    }

    #[tokio::test]
    async fn test_frost_signing_with_middleware() {
        let effects = Effects::test(42);
        let device_ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();

        let participants: Vec<BridgedRole> = device_ids
            .iter()
            .enumerate()
            .map(|(i, &id)| BridgedRole {
                device_id: id,
                role_index: i,
            })
            .collect();

        let handler = EffectsMiddleware::new(InMemoryHandler::new(), effects.clone());

        let context = BaseContext::new(device_ids[0], effects.clone(), None, None);

        let mut adapter = RumpsteakAdapter::new(
            handler,
            AuraEffectsAdapter::new(device_ids[0], effects.clone()),
            context,
        );

        let mut endpoint = BridgedEndpoint {
            context: BaseContext::new(device_ids[0], effects.clone(), None, None),
        };

        // Run FROST signing
        let message = b"test message".to_vec();
        let frost = FrostSigningProtocol::new(participants.clone(), message);

        let result = frost
            .execute(&mut adapter, &mut endpoint, participants[0])
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 64);
    }
}
