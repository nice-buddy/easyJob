use easyjob_ipc::protocol::{AgentStatus, IpcEvent, IpcMessage, IpcRequest, IpcResponse};

#[test]
fn test_request_response_serialization() {
    let req = IpcRequest::new("task.list", serde_json::json!({}));
    let json = serde_json::to_string(&req).unwrap();
    let deserialized: IpcRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req.id, deserialized.id);
    assert_eq!(req.method, "task.list");

    let resp = IpcResponse::success(req.id.clone(), serde_json::json!({ "count": 10 }));
    let resp_json = serde_json::to_string(&resp).unwrap();
    let deserialized_resp: IpcResponse = serde_json::from_str(&resp_json).unwrap();
    assert!(deserialized_resp.ok);
    assert_eq!(deserialized_resp.id, req.id);

    let err_resp = IpcResponse::error(req.id.clone(), "task not found");
    let err_json = serde_json::to_string(&err_resp).unwrap();
    let deserialized_err: IpcResponse = serde_json::from_str(&err_json).unwrap();
    assert!(!deserialized_err.ok);
    assert_eq!(deserialized_err.error, Some("task not found".to_string()));
    assert_eq!(deserialized_err.data, None);

    let event = IpcEvent::new("execution.started", serde_json::json!({ "id": "123" }));
    let msg = IpcMessage::Event(event.clone());
    let msg_json = serde_json::to_string(&msg).unwrap();
    assert!(msg_json.contains("execution.started"));

    // Verify IpcMessage untagged deserialization
    let msg_event: IpcMessage = serde_json::from_str(&msg_json).unwrap();
    assert_eq!(msg_event, IpcMessage::Event(event));

    let msg_req = IpcMessage::Request(req.clone());
    let msg_req_json = serde_json::to_string(&msg_req).unwrap();
    let deserialized_msg_req: IpcMessage = serde_json::from_str(&msg_req_json).unwrap();
    assert_eq!(deserialized_msg_req, IpcMessage::Request(req));

    let msg_resp = IpcMessage::Response(resp.clone());
    let msg_resp_json = serde_json::to_string(&msg_resp).unwrap();
    let deserialized_msg_resp: IpcMessage = serde_json::from_str(&msg_resp_json).unwrap();
    assert_eq!(deserialized_msg_resp, IpcMessage::Response(resp));
}

#[test]
fn test_agent_status_serialization() {
    let status = AgentStatus {
        version: "0.1.0".to_string(),
        uptime_secs: 120,
        active_tasks: 5,
        running_executions: 1,
    };
    let json = serde_json::to_string(&status).unwrap();
    let parsed: AgentStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.uptime_secs, 120);
    assert_eq!(parsed, status);
}
