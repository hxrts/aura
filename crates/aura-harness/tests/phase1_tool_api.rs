#![allow(missing_docs)]

use aura_harness::config::{InstanceConfig, InstanceMode, RunConfig, RunSection, ScreenSource};
use aura_harness::coordinator::HarnessCoordinator;
use aura_harness::tool_api::{ToolApi, ToolKey, ToolRequest, ToolResponse};

#[test]
fn tool_api_primitives_control_local_pty_instance() {
    let temp_root = std::env::temp_dir().join("aura-harness-phase1");
    let _ = std::fs::create_dir_all(&temp_root);
    let log_path = temp_root.join("alice.log");
    let _ = std::fs::write(&log_path, "line-1\nline-2\nline-3\n");

    let run_config = RunConfig {
        schema_version: 1,
        run: RunSection {
            name: "phase1-tool-api".to_string(),
            pty_rows: Some(40),
            pty_cols: Some(120),
            artifact_dir: None,
            global_budget_ms: None,
            step_budget_ms: None,
            seed: Some(1),
            max_cpu_percent: None,
            max_memory_bytes: None,
            max_open_files: None,
            require_remote_artifact_sync: false,
        },
        instances: vec![InstanceConfig {
            id: "alice".to_string(),
            mode: InstanceMode::Local,
            data_dir: temp_root.join("alice-data"),
            device_id: None,
            bind_address: "127.0.0.1:41001".to_string(),
            demo_mode: false,
            command: Some("bash".to_string()),
            args: vec!["-lc".to_string(), "cat".to_string()],
            env: vec![],
            log_path: Some(log_path),
            ssh_host: None,
            ssh_user: None,
            ssh_port: None,
            ssh_strict_host_key_checking: true,
            ssh_known_hosts_file: None,
            ssh_fingerprint: None,
            ssh_require_fingerprint: false,
            ssh_dry_run: true,
            remote_workdir: None,
            lan_discovery: None,
            tunnel: None,
        }],
    };

    let coordinator = match HarnessCoordinator::from_run_config(&run_config) {
        Ok(coordinator) => coordinator,
        Err(error) => panic!("coordinator init failed: {error}"),
    };
    let mut tool_api = ToolApi::new(coordinator);
    if let Err(error) = tool_api.start_all() {
        panic!("start_all failed: {error}");
    }

    match tool_api.handle_request(ToolRequest::SendKeys {
        instance_id: "alice".to_string(),
        keys: "hello-pty\n".to_string(),
    }) {
        ToolResponse::Ok { .. } => {}
        ToolResponse::Error { message } => panic!("send_keys failed: {message}"),
    }

    match tool_api.handle_request(ToolRequest::WaitFor {
        instance_id: "alice".to_string(),
        pattern: "hello-pty".to_string(),
        timeout_ms: 2000,
        screen_source: ScreenSource::Default,
    }) {
        ToolResponse::Ok { .. } => {}
        ToolResponse::Error { message } => panic!("wait_for failed: {message}"),
    }

    match tool_api.handle_request(ToolRequest::SendKeys {
        instance_id: "alice".to_string(),
        keys: "hello-key".to_string(),
    }) {
        ToolResponse::Ok { .. } => {}
        ToolResponse::Error { message } => panic!("send_keys hello-key failed: {message}"),
    }
    match tool_api.handle_request(ToolRequest::SendKey {
        instance_id: "alice".to_string(),
        key: ToolKey::Enter,
        repeat: 1,
    }) {
        ToolResponse::Ok { .. } => {}
        ToolResponse::Error { message } => panic!("send_key enter failed: {message}"),
    }
    match tool_api.handle_request(ToolRequest::WaitFor {
        instance_id: "alice".to_string(),
        pattern: "hello-key".to_string(),
        timeout_ms: 2000,
        screen_source: ScreenSource::Default,
    }) {
        ToolResponse::Ok { .. } => {}
        ToolResponse::Error { message } => panic!("wait_for hello-key failed: {message}"),
    }

    let screen_payload = match tool_api.handle_request(ToolRequest::Screen {
        instance_id: "alice".to_string(),
        screen_source: ScreenSource::Default,
    }) {
        ToolResponse::Ok { payload } => payload,
        ToolResponse::Error { message } => panic!("screen failed: {message}"),
    };
    let screen_text = screen_payload
        .get("screen")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let authoritative_screen_text = screen_payload
        .get("authoritative_screen")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let raw_screen_text = screen_payload
        .get("raw_screen")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    assert_eq!(screen_text, authoritative_screen_text);
    assert!(screen_text.contains("hello-pty"));
    assert!(raw_screen_text.contains("hello-pty"));

    let tail_payload = match tool_api.handle_request(ToolRequest::TailLog {
        instance_id: "alice".to_string(),
        lines: 2,
    }) {
        ToolResponse::Ok { payload } => payload,
        ToolResponse::Error { message } => panic!("tail_log failed: {message}"),
    };
    let lines = tail_payload
        .get("lines")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(lines.len(), 2);

    match tool_api.handle_request(ToolRequest::Restart {
        instance_id: "alice".to_string(),
    }) {
        ToolResponse::Ok { .. } => {}
        ToolResponse::Error { message } => panic!("restart failed: {message}"),
    }

    match tool_api.handle_request(ToolRequest::Kill {
        instance_id: "alice".to_string(),
    }) {
        ToolResponse::Ok { .. } => {}
        ToolResponse::Error { message } => panic!("kill failed: {message}"),
    }

    if let Err(error) = tool_api.stop_all() {
        panic!("stop_all failed: {error}");
    }

    let event_ops: Vec<String> = tool_api
        .event_snapshot()
        .into_iter()
        .map(|event| event.operation)
        .collect();
    assert!(event_ops.iter().any(|operation| operation == "start"));
    assert!(event_ops.iter().any(|operation| operation == "send_keys"));
    assert!(event_ops.iter().any(|operation| operation == "wait_for"));
    assert!(event_ops.iter().any(|operation| operation == "kill"));
}

#[test]
fn artifact_bundle_writes_json_payloads() {
    let temp_root = std::env::temp_dir().join("aura-harness-artifacts-test");
    let _ = std::fs::create_dir_all(&temp_root);
    let bundle = match aura_harness::artifacts::ArtifactBundle::create(&temp_root, "phase1") {
        Ok(bundle) => bundle,
        Err(error) => panic!("bundle creation failed: {error}"),
    };

    let payload = serde_json::json!({ "ok": true, "name": "phase1" });
    let file_path = match bundle.write_json("payload.json", &payload) {
        Ok(path) => path,
        Err(error) => panic!("write_json failed: {error}"),
    };

    let body = match std::fs::read_to_string(&file_path) {
        Ok(body) => body,
        Err(error) => panic!("read failed: {error}"),
    };
    assert!(body.contains("phase1"));
}

#[test]
fn tool_request_json_round_trip() {
    let request = ToolRequest::SendKeys {
        instance_id: "alice".to_string(),
        keys: "abc".to_string(),
    };
    let encoded = match serde_json::to_string(&request) {
        Ok(encoded) => encoded,
        Err(error) => panic!("encode failed: {error}"),
    };
    let decoded: ToolRequest = match serde_json::from_str(&encoded) {
        Ok(decoded) => decoded,
        Err(error) => panic!("decode failed: {error}"),
    };

    match decoded {
        ToolRequest::SendKeys { instance_id, keys } => {
            assert_eq!(instance_id, "alice");
            assert_eq!(keys, "abc");
        }
        _ => panic!("decoded request variant mismatch"),
    }
}

#[test]
fn tool_request_send_key_round_trip() {
    let request = ToolRequest::SendKey {
        instance_id: "alice".to_string(),
        key: ToolKey::Esc,
        repeat: 2,
    };
    let encoded = match serde_json::to_string(&request) {
        Ok(encoded) => encoded,
        Err(error) => panic!("encode failed: {error}"),
    };
    let decoded: ToolRequest = match serde_json::from_str(&encoded) {
        Ok(decoded) => decoded,
        Err(error) => panic!("decode failed: {error}"),
    };

    match decoded {
        ToolRequest::SendKey {
            instance_id,
            key,
            repeat,
        } => {
            assert_eq!(instance_id, "alice");
            assert!(matches!(key, ToolKey::Esc));
            assert_eq!(repeat, 2);
        }
        _ => panic!("decoded request variant mismatch"),
    }
}
