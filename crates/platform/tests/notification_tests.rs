use easyjob_platform::notification::send_system_notification;

#[tokio::test]
async fn test_send_system_notification_smoke() {
    // 验证调用接口传参转义安全且不抛 panic
    let res = send_system_notification(
        "easyJob Test",
        Some("Subtitle \"Quotes\""),
        "Body with \\ and \n",
    )
    .await;
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_send_system_notification_without_subtitle() {
    let res = send_system_notification("easyJob Test", None, "Body without subtitle").await;
    assert!(res.is_ok());
}
