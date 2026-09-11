use easyjob_common::{
    error::{Error, Result},
    id::{ActionId, ExecutionId, TaskId, TriggerId},
    time::{duration_between_utc, duration_ms_between, now_utc},
};
use uuid::Uuid;

#[test]
fn test_id_generation_and_serde() {
    let task_id = TaskId::new();
    let json = serde_json::to_string(&task_id).unwrap();
    let deserialized: TaskId = serde_json::from_str(&json).unwrap();
    assert_eq!(task_id, deserialized);
    assert!(!task_id.to_string().is_empty());

    let trigger_id = TriggerId::new();
    let action_id = ActionId::new();
    let exec_id = ExecutionId::new();
    assert_ne!(trigger_id.to_string(), action_id.to_string());
    assert_ne!(action_id.to_string(), exec_id.to_string());
}

#[test]
fn test_id_methods_and_traits() {
    let raw_uuid = Uuid::new_v4();
    let task_id = TaskId::from_uuid(raw_uuid);
    assert_eq!(task_id.as_uuid(), &raw_uuid);
    assert_eq!(task_id.to_string(), raw_uuid.to_string());

    let parsed = TaskId::parse(&raw_uuid.to_string()).expect("valid uuid string");
    assert_eq!(task_id, parsed);

    let invalid_parse = TaskId::parse("not-a-uuid");
    assert!(invalid_parse.is_err());

    let default_id = TaskId::default();
    assert!(!default_id.to_string().is_empty());
}

#[test]
fn test_error_variants() {
    let val_err = Error::Validation("invalid name".to_string());
    assert_eq!(val_err.to_string(), "Validation error: invalid name");

    let not_found = Error::NotFound("task 123".to_string());
    assert_eq!(not_found.to_string(), "Not found: task 123");

    let io_err: Error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found").into();
    assert!(matches!(io_err, Error::Io(_)));

    let json_err: Error = serde_json::from_str::<TaskId>("invalid-json")
        .unwrap_err()
        .into();
    assert!(matches!(json_err, Error::Serialization(_)));

    fn dummy_result() -> Result<TaskId> {
        Err(Error::Other("custom error".to_string()))
    }
    assert!(dummy_result().is_err());
}

#[test]
fn test_time_utilities() {
    let t1 = now_utc();
    let t2 = t1 + chrono::Duration::milliseconds(150);
    assert_eq!(duration_ms_between(t1, t2), 150);
    assert_eq!(duration_between_utc(t1, t2), 150);

    // Negative duration should clamp to 0
    assert_eq!(duration_ms_between(t2, t1), 0);
    assert_eq!(duration_between_utc(t2, t1), 0);
}
