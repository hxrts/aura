//! Native host for real browser-to-production-broker integration evidence.
#[cfg(not(target_arch = "wasm32"))]
async fn bind_fixture_broker(
    auth_token: &str,
) -> Result<aura_agent::LocalBootstrapBrokerService, String> {
    let broker = aura_agent::LocalBootstrapBrokerService::bind(
        "127.0.0.1:0",
        std::time::Duration::from_secs(
            aura_agent::BootstrapBrokerConfig::default().registration_ttl_secs,
        ),
        auth_token,
        aura_agent::BootstrapBrokerLanBindPolicy::LoopbackOnly,
    )
    .await?;
    Ok(broker.with_mailbox_time_provider(std::sync::Arc::new(
        aura_effects::time::PhysicalTimeHandler::new(),
    )))
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use aura_agent::TaskSupervisor;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let auth_token = std::env::var("AURA_BROWSER_BROKER_AUTH_TOKEN")?;
    if auth_token.len() < 32 || !auth_token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "broker fixture requires a 32+ character hex authorization token",
        )
        .into());
    }

    let broker = bind_fixture_broker(&auth_token)
        .await
        .map_err(std::io::Error::other)?;
    let control = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let supervisor = TaskSupervisor::new();
    broker.start(&supervisor.group("browser_broker_fixture"));
    println!(
        "{}",
        serde_json::json!({
            "broker_url": broker.public_url(),
            "control_port": control.local_addr()?.port(),
        })
    );
    let (mut stream, _) = control.accept().await?;
    let mut command = [0; 5];
    stream.read_exact(&mut command).await?;
    if command != *b"STOP\n" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "expected fixture shutdown request",
        )
        .into());
    }
    supervisor
        .shutdown_with_timeout(Duration::from_secs(5))
        .await?;
    stream.write_all(b"DISPOSED\n").await?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {
    panic!("the browser broker host fixture requires a native process");
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn read_return_registration(
        broker: &aura_agent::LocalBootstrapBrokerService,
        auth: &str,
    ) -> String {
        let addr = broker.public_url().trim_start_matches("http://");
        let mut stream = tokio::net::TcpStream::connect(addr)
            .await
            .expect("broker connection");
        let path = format!(
            "/v2/bootstrap/enrollment-return-mailbox/register/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee/{}/{}",
            aura_core::DeviceId::from_bytes([7; 32]),
            hex::encode([9; 32]),
        );
        stream
            .write_all(
                format!(
                    "GET {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {auth}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .expect("request write");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .expect("response read");
        response
    }

    #[tokio::test]
    async fn fixture_return_mailbox_requires_time_and_bearer_auth() {
        const AUTH: &str = "0123456789abcdef0123456789abcdef";
        let broker = bind_fixture_broker(AUTH)
            .await
            .expect("fixture broker bind");
        let supervisor = aura_agent::TaskSupervisor::new();
        broker.start(&supervisor.group("fixture_return_mailbox"));
        tokio::task::yield_now().await;

        let authorized = read_return_registration(&broker, AUTH).await;
        assert!(authorized.starts_with("HTTP/1.1 200 "), "{authorized}");
        assert!(authorized.ends_with("null"), "{authorized}");
        let refused = read_return_registration(&broker, "wrong-token").await;
        assert!(refused.starts_with("HTTP/1.1 401 "), "{refused}");
        supervisor
            .shutdown_with_timeout(std::time::Duration::from_secs(5))
            .await
            .expect("fixture broker shutdown");
    }
}
