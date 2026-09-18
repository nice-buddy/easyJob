use std::sync::Arc;

use easyjob_domain::trigger::{NetworkEventKind, TriggerKind};
use easyjob_persistence::task_repo::{SqliteTaskRepository, TaskRepository};
use easyjob_scheduler::scheduler::SchedulerCommand;
use tokio::sync::mpsc;
use tracing::warn;

/// 运行时网络事件（含事件发生时的 SSID；有线或读取失败为 None）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkEvent {
    Connect { ssid: Option<String> },
    Disconnect { ssid: Option<String> },
    Online { ssid: Option<String> },
}

impl NetworkEvent {
    pub fn kind(&self) -> NetworkEventKind {
        match self {
            NetworkEvent::Connect { .. } => NetworkEventKind::Connect,
            NetworkEvent::Disconnect { .. } => NetworkEventKind::Disconnect,
            NetworkEvent::Online { .. } => NetworkEventKind::Online,
        }
    }

    pub fn ssid(&self) -> Option<&str> {
        match self {
            NetworkEvent::Connect { ssid }
            | NetworkEvent::Disconnect { ssid }
            | NetworkEvent::Online { ssid } => ssid.as_deref(),
        }
    }
}

/// 事件类型匹配：触发器 events 列表非空且包含事件类型
pub fn event_matches(ev: &NetworkEvent, events: &[NetworkEventKind]) -> bool {
    events.contains(&ev.kind())
}

/// SSID 过滤：network_name 为 None = 任意网络；Some(name) = 精确、区分大小写匹配
pub fn name_matches(ev: &NetworkEvent, network_name: &Option<String>) -> bool {
    match network_name {
        None => true,
        Some(expected) => ev.ssid() == Some(expected.as_str()),
    }
}

/// 单个 TCP 地址可达性探测：2s 超时，且内层 connect 结果必须为 Ok
async fn tcp_reachable(addr: &str) -> bool {
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::net::TcpStream::connect(addr),
    )
    .await
    .map(|res| res.is_ok())
    .unwrap_or(false)
}

/// 共享 Online 探测：TCP 1.1.1.1:80（2s 超时），失败 fallback DNS 解析 example.com
pub async fn probe_online() -> bool {
    if tcp_reachable("1.1.1.1:80").await {
        return true;
    }
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::net::lookup_host("example.com:443"),
    )
    .await
    .map(|resolved| {
        resolved
            .map(|mut addrs| addrs.next().is_some())
            .unwrap_or(false)
    })
    .unwrap_or(false)
}

/// 派发：查启用任务 → 匹配 Network 触发器（事件类型 + SSID）→ TriggerNow（每任务至多一次）
pub async fn dispatch_network_event(
    ev: NetworkEvent,
    task_repo: Arc<SqliteTaskRepository>,
    scheduler_tx: mpsc::Sender<SchedulerCommand>,
) {
    let Ok(tasks) = task_repo.find_all_enabled().await else {
        warn!("network dispatch: failed to list enabled tasks");
        return;
    };
    for task in tasks {
        let hit = task.triggers.iter().any(|tr| {
            tr.enabled
                && match &tr.kind {
                    TriggerKind::Network {
                        events,
                        network_name,
                    } => event_matches(&ev, events) && name_matches(&ev, network_name),
                    _ => false,
                }
        });
        if hit {
            let _ = scheduler_tx
                .send(SchedulerCommand::TriggerNow(task.id))
                .await;
        }
    }
}

/// 启动平台网络监控。事件经 event_tx 发出（try_send，通道满则丢弃并 warn）。
/// 返回 JoinHandle；handler 退出即停止监控。
pub fn spawn_network_monitor(event_tx: mpsc::Sender<NetworkEvent>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let (tx, mut rx) = mpsc::unbounded_channel::<NetworkEvent>();
        let send = move |ev: NetworkEvent| {
            if tx.send(ev).is_err() {
                warn!("network monitor: internal channel closed");
            }
        };

        #[cfg(target_os = "macos")]
        let platform = platform_macos::MacosNetworkWatcher::start(send);
        #[cfg(target_os = "windows")]
        let platform = platform_windows::WindowsNetworkWatcher::start(send);

        tracing::info!("network monitor started");
        while let Some(ev) = rx.recv().await {
            if let Err(e) = event_tx.try_send(ev) {
                tracing::warn!("network event dropped (channel full): {}", e);
            }
        }
        drop(platform);
        tracing::info!("network monitor stopped");
    })
}

/// 防抖共享状态：同目标事件 500ms 内去重
pub(crate) struct Debouncer {
    last_kind: std::sync::Mutex<Option<(NetworkEventKind, std::time::Instant)>>,
}

impl Debouncer {
    pub fn new() -> Self {
        Self {
            last_kind: std::sync::Mutex::new(None),
        }
    }

    /// 返回 true = 放行
    pub fn allow(&self, kind: NetworkEventKind) -> bool {
        let mut guard = self.last_kind.lock().unwrap();
        let now = std::time::Instant::now();
        match guard.as_mut() {
            Some((last, at)) if *last == kind && now.duration_since(*at).as_millis() < 500 => false,
            _ => {
                *guard = Some((kind, now));
                true
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod platform_macos {
    use super::{Debouncer, NetworkEvent};
    use block2::{Block, RcBlock};
    use std::ffi::{c_char, c_void};
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
    use std::sync::Arc;
    use tracing::info;

    type SendFn = Arc<dyn Fn(NetworkEvent) + Send + Sync>;

    // Network.framework 为纯 C API（objc2-network 仅是占位 crate，无实际绑定），
    // 因此直接声明所需的最小符号集。
    #[link(name = "Network", kind = "framework")]
    extern "C" {
        fn nw_path_monitor_create() -> *mut c_void;
        fn nw_path_monitor_set_queue(monitor: *mut c_void, queue: *mut c_void);
        fn nw_path_monitor_set_update_handler(
            monitor: *mut c_void,
            update_handler: &Block<dyn Fn(*mut c_void)>,
        );
        fn nw_path_monitor_start(monitor: *mut c_void);
        fn nw_path_monitor_cancel(monitor: *mut c_void);
        fn nw_path_get_status(path: *mut c_void) -> i32;
    }

    // dispatch 属于 libSystem，无需额外 link
    extern "C" {
        fn dispatch_queue_create(label: *const c_char, attr: *const c_void) -> *mut c_void;
    }

    // nw_path_status_t：1 = Satisfied，2 = Unsatisfied
    const NW_PATH_STATUS_SATISFIED: i32 = 1;
    const NW_PATH_STATUS_UNSATISFIED: i32 = 2;

    pub struct MacosNetworkWatcher {
        monitor: *mut c_void,
        queue: *mut c_void,
        running: Arc<AtomicBool>,
    }

    // nw_* 与 dispatch queue 均线程安全
    unsafe impl Send for MacosNetworkWatcher {}
    unsafe impl Sync for MacosNetworkWatcher {}

    impl MacosNetworkWatcher {
        pub fn start<F>(on_event: F) -> Self
        where
            F: Fn(NetworkEvent) + Send + Sync + 'static,
        {
            let on_event: SendFn = Arc::new(on_event);
            let running = Arc::new(AtomicBool::new(true));
            let debouncer = Arc::new(Debouncer::new());
            // 回调在 GCD 线程触发，那里没有 Tokio 上下文，故在此处（Tokio 任务内）取 Handle
            let probe_handle = tokio::runtime::Handle::try_current().ok();

            let cb_running = running.clone();
            let cb_debounce = debouncer.clone();
            // start 时 update handler 会立即用当前 path 触发一次快照回调（非状态变迁），
            // 与 Windows 侧 InitialNotification=false 对齐，故整帧跳过第一次回调。
            let first_call = Arc::new(AtomicBool::new(true));
            let cb_first = first_call.clone();
            let cb = RcBlock::new(move |path: *mut c_void| {
                if !cb_running.load(Ordering::Relaxed) {
                    return;
                }
                if cb_first.swap(false, Ordering::Relaxed) {
                    return;
                }
                let status = unsafe { nw_path_get_status(path) };
                let event = match status {
                    NW_PATH_STATUS_SATISFIED => NetworkEvent::Connect {
                        ssid: current_ssid(),
                    },
                    NW_PATH_STATUS_UNSATISFIED => NetworkEvent::Disconnect {
                        ssid: current_ssid(),
                    },
                    _ => return, // Satisfiable / Invalid 等中间态不触发
                };
                if !cb_debounce.allow(event.kind()) {
                    return;
                }
                let is_connect = matches!(event, NetworkEvent::Connect { .. });
                info!("network path changed: {:?}", event);
                on_event(event);
                // Connect 后异步探测 Online（10s 冷却在探测任务内部）
                if is_connect {
                    spawn_online_probe(on_event.clone(), cb_debounce.clone(), probe_handle.clone());
                }
            });

            unsafe {
                let monitor = nw_path_monitor_create();
                // start 前必须指定队列，否则回调投递到主队列（agent 无主队列循环）
                let queue =
                    dispatch_queue_create(c"easyjob.network-monitor".as_ptr(), std::ptr::null());
                nw_path_monitor_set_queue(monitor, queue);
                nw_path_monitor_set_update_handler(monitor, &cb);
                nw_path_monitor_start(monitor);
                Self {
                    monitor,
                    queue,
                    running,
                }
            }
        }

        pub fn stop(&self) {
            self.running.store(false, Ordering::Relaxed);
            unsafe { nw_path_monitor_cancel(self.monitor) };
        }
    }

    impl Drop for MacosNetworkWatcher {
        fn drop(&mut self) {
            self.stop();
            // nw_path_monitor_create / dispatch_queue_create 均为 +1，需自行释放
            unsafe {
                objc2::ffi::objc_release(self.monitor.cast::<objc2::runtime::AnyObject>());
                objc2::ffi::objc_release(self.queue.cast::<objc2::runtime::AnyObject>());
            }
        }
    }

    fn current_ssid() -> Option<String> {
        // macOS 14+ 读取 SSID 需定位权限；无权限/有线时返回 None（触发器按任意网络匹配）
        unsafe {
            let client = objc2_core_wlan::CWWiFiClient::sharedWiFiClient();
            let iface = client.interface()?;
            iface.ssid().map(|s| s.to_string())
        }
    }

    fn spawn_online_probe(
        on_event: SendFn,
        debouncer: Arc<Debouncer>,
        probe_handle: Option<tokio::runtime::Handle>,
    ) {
        static LAST_PROBE_MS: AtomicI64 = AtomicI64::new(0);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if now_ms - LAST_PROBE_MS.load(Ordering::Relaxed) < 10_000 {
            return; // 10s 探测冷却
        }
        LAST_PROBE_MS.store(now_ms, Ordering::Relaxed);

        let Some(handle) = probe_handle else {
            return;
        };
        handle.spawn(async move {
            if super::probe_online().await {
                let ev = NetworkEvent::Online {
                    ssid: current_ssid(),
                };
                if debouncer.allow(ev.kind()) {
                    on_event(ev);
                }
            }
        });
    }
}

#[cfg(target_os = "windows")]
mod platform_windows {
    use super::{Debouncer, NetworkEvent};
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
    use std::sync::Arc;
    use tracing::{error, info};
    use windows::Win32::Foundation::{HANDLE, WIN32_ERROR};
    use windows::Win32::NetworkManagement::IpHelper::{
        MibDeleteInstance, NotifyIpInterfaceChange, MIB_IPINTERFACE_ROW, MIB_NOTIFICATION_TYPE,
    };
    use windows::Win32::NetworkManagement::WiFi::{
        wlan_intf_opcode_current_connection, wlan_notification_acm_connection_complete,
        wlan_notification_acm_disconnected, WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory,
        WlanOpenHandle, WlanQueryInterface, WlanRegisterNotification, L2_NOTIFICATION_DATA,
        WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST, WLAN_NOTIFICATION_CALLBACK,
        WLAN_NOTIFICATION_SOURCE_ACM,
    };
    use windows::Win32::Networking::WinSock::AF_UNSPEC;

    type SendFn = Arc<dyn Fn(NetworkEvent) + Send + Sync>;

    pub struct WindowsNetworkWatcher {
        state: Arc<WatcherState>,
    }

    struct WatcherState {
        running: Arc<AtomicBool>,
        on_event: SendFn,
        debouncer: Arc<Debouncer>,
        // 回调在系统线程触发，那里没有 Tokio 上下文，故在 start() 内取 Handle
        probe_handle: Option<tokio::runtime::Handle>,
    }

    impl WindowsNetworkWatcher {
        pub fn start<F>(on_event: F) -> Self
        where
            F: Fn(NetworkEvent) + Send + Sync + 'static,
        {
            let state = Arc::new(WatcherState {
                running: Arc::new(AtomicBool::new(true)),
                on_event: Arc::new(on_event),
                debouncer: Arc::new(Debouncer::new()),
                probe_handle: tokio::runtime::Handle::try_current().ok(),
            });

            // 1) IP 接口变动（有线 + 通用 up/down）
            start_interface_watch(state.clone());

            // 2) WLAN 专用通知（提供 SSID）
            start_wlan_watch(state.clone());

            Self { state }
        }

        pub fn stop(&self) {
            self.state.running.store(false, Ordering::Relaxed);
        }
    }

    impl Drop for WindowsNetworkWatcher {
        fn drop(&mut self) {
            self.stop();
        }
    }

    fn start_interface_watch(state: Arc<WatcherState>) {
        // 泄漏一个强引用作为回调上下文，保证进程生命周期内始终有效
        let ctx = Arc::into_raw(state).cast::<c_void>();
        let mut handle = HANDLE::default();
        let res = unsafe {
            NotifyIpInterfaceChange(AF_UNSPEC, Some(ip_callback), Some(ctx), false, &mut handle)
        };
        if res != WIN32_ERROR(0) {
            error!("NotifyIpInterfaceChange failed: {}", res.0);
            // 注册失败则回收上面泄漏的强引用（成功时保持泄漏，供系统回调长期使用）
            unsafe { drop(Arc::from_raw(ctx as *const WatcherState)) };
        }
    }

    unsafe extern "system" fn ip_callback(
        caller_context: *const c_void,
        row: *const MIB_IPINTERFACE_ROW,
        notification_type: MIB_NOTIFICATION_TYPE,
    ) {
        // 借用泄漏的强引用（不取得所有权，不改变引用计数）
        let state = std::mem::ManuallyDrop::new(unsafe {
            Arc::from_raw(caller_context as *const WatcherState)
        });
        // MIB_IPINTERFACE_ROW 没有 OperStatus，只有 Connected（是否已连到网络接入点）。
        // MibDeleteInstance = 接口行被移除；其余通知必须按 Connected 判定，
        // 否则拔网线 / 禁用网卡（参数变更且 Connected=false）会被误报成 Connect。
        //
        // 已知残留：接口仍连接时的纯参数变更（MTU / metric / DNS）Connected 仍为 true，
        // 会误报一次 Connect。彻底消除需按 InterfaceLuid 维护状态迁移表，本次不做。
        let connected = !row.is_null() && unsafe { (*row).Connected };
        let event = if notification_type == MibDeleteInstance || !connected {
            NetworkEvent::Disconnect { ssid: None }
        } else {
            NetworkEvent::Connect {
                ssid: query_current_ssid(),
            }
        };
        emit(&state, event);
    }

    fn start_wlan_watch(state: Arc<WatcherState>) {
        let callback: WLAN_NOTIFICATION_CALLBACK = Some(wlan_callback);
        std::thread::spawn(move || unsafe {
            let mut negotiated = 0u32;
            let mut handle = HANDLE::default();
            if WlanOpenHandle(2, None, &mut negotiated, &mut handle) != 0 {
                error!("WlanOpenHandle failed (WLAN service unavailable?)");
                return;
            }
            // dwPrevNotif: NULL；注册全部 ACM 通知
            let _ = WlanRegisterNotification(
                handle,
                WLAN_NOTIFICATION_SOURCE_ACM,
                false,
                callback,
                Some(Arc::as_ptr(&state).cast::<c_void>()),
                None,
                None,
            );
            // WLAN 回调在系统线程触发；此线程挂起等待 stop
            while state.running_flag() {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            let _ = WlanCloseHandle(handle, None);
        });
    }

    unsafe extern "system" fn wlan_callback(
        notification: *mut L2_NOTIFICATION_DATA,
        context: *mut c_void,
    ) {
        if notification.is_null() {
            return;
        }
        let n = unsafe { &*notification };
        if n.NotificationSource != WLAN_NOTIFICATION_SOURCE_ACM {
            return;
        }
        // 上下文为 start_wlan_watch 中 Arc::as_ptr 的借用指针（state 由该线程持有）
        let state = unsafe { &*(context as *const WatcherState) };
        let event = if n.NotificationCode == wlan_notification_acm_connection_complete.0 as u32 {
            NetworkEvent::Connect {
                ssid: query_current_ssid(),
            }
        } else if n.NotificationCode == wlan_notification_acm_disconnected.0 as u32 {
            NetworkEvent::Disconnect {
                ssid: query_current_ssid(),
            }
        } else {
            return;
        };
        emit(state, event);
    }

    impl WatcherState {
        fn running_flag(&self) -> bool {
            self.running.load(Ordering::Relaxed)
        }
    }

    fn emit(state: &WatcherState, event: NetworkEvent) {
        if !state.debouncer.allow(event.kind()) {
            return;
        }
        let is_connect = matches!(event, NetworkEvent::Connect { .. });
        info!("network event: {:?}", event);
        (state.on_event)(event);
        if is_connect {
            spawn_online_probe(state);
        }
    }

    fn spawn_online_probe(state: &WatcherState) {
        static LAST_PROBE_MS: AtomicI64 = AtomicI64::new(0);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if now_ms - LAST_PROBE_MS.load(Ordering::Relaxed) < 10_000 {
            return; // 10s 探测冷却
        }
        LAST_PROBE_MS.store(now_ms, Ordering::Relaxed);

        let on_event = state.on_event.clone();
        let debouncer = state.debouncer.clone();
        let Some(handle) = state.probe_handle.clone() else {
            return;
        };
        handle.spawn(async move {
            if super::probe_online().await {
                let ev = NetworkEvent::Online {
                    ssid: query_current_ssid(),
                };
                if debouncer.allow(ev.kind()) {
                    on_event(ev);
                }
            }
        });
    }

    /// 查询当前关联 SSID（非 WiFi 或失败 → None）
    fn query_current_ssid() -> Option<String> {
        unsafe {
            let mut negotiated = 0u32;
            let mut handle = HANDLE::default();
            if WlanOpenHandle(2, None, &mut negotiated, &mut handle) != 0 {
                return None;
            }
            let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            if WlanEnumInterfaces(handle, None, &mut list) != 0 || list.is_null() {
                let _ = WlanCloseHandle(handle, None);
                return None;
            }
            let info_list = &*list;
            let interfaces = std::slice::from_raw_parts(
                info_list.InterfaceInfo.as_ptr(),
                info_list.dwNumberOfItems as usize,
            );
            let mut ssid = None;
            for info in interfaces {
                let mut data_size = 0u32;
                let mut data: *mut c_void = std::ptr::null_mut();
                if WlanQueryInterface(
                    handle,
                    &info.InterfaceGuid,
                    wlan_intf_opcode_current_connection,
                    None,
                    &mut data_size,
                    &mut data,
                    None,
                ) == 0
                    && !data.is_null()
                {
                    let conn = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
                    let raw = &conn.wlanAssociationAttributes.dot11Ssid;
                    // clamp 到数组实际长度：越界切片会在 FFI 回调内 panic 并 abort 进程
                    let len = (raw.uSSIDLength as usize).min(raw.ucSSID.len());
                    if len > 0 {
                        ssid = Some(String::from_utf8_lossy(&raw.ucSSID[..len]).to_string());
                    }
                    WlanFreeMemory(data as *const c_void);
                    if ssid.is_some() {
                        break;
                    }
                }
            }
            WlanFreeMemory(list as *const c_void);
            let _ = WlanCloseHandle(handle, None);
            ssid
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use easyjob_common::{ActionId, TaskId, TriggerId};
    use easyjob_domain::action::{Action, ActionKind};
    use easyjob_domain::policy::ExecutionPolicy;
    use easyjob_domain::task::Task;
    use easyjob_domain::trigger::{NetworkEventKind, Trigger};
    use easyjob_persistence::db::init_pool;
    use std::collections::HashMap;

    fn connect(ssid: Option<&str>) -> NetworkEvent {
        NetworkEvent::Connect {
            ssid: ssid.map(String::from),
        }
    }

    #[test]
    fn event_kind_maps_correctly() {
        assert_eq!(connect(Some("x")).kind(), NetworkEventKind::Connect);
        assert_eq!(
            NetworkEvent::Disconnect { ssid: None }.kind(),
            NetworkEventKind::Disconnect
        );
        assert_eq!(
            NetworkEvent::Online { ssid: None }.kind(),
            NetworkEventKind::Online
        );
    }

    #[test]
    fn event_matches_respects_list() {
        let ev = connect(Some("x"));
        assert!(event_matches(&ev, &[NetworkEventKind::Connect]));
        assert!(event_matches(
            &ev,
            &[NetworkEventKind::Disconnect, NetworkEventKind::Connect]
        ));
        assert!(!event_matches(&ev, &[NetworkEventKind::Disconnect]));
        assert!(!event_matches(&ev, &[]));
    }

    #[test]
    fn name_matches_none_is_wildcard() {
        assert!(name_matches(&connect(Some("Home")), &None));
        assert!(name_matches(&connect(None), &None));
    }

    #[test]
    fn name_matches_requires_exact_case_sensitive_ssid() {
        let name = Some("MyHome".to_string());
        assert!(name_matches(&connect(Some("MyHome")), &name));
        assert!(!name_matches(&connect(Some("myhome")), &name));
        assert!(!name_matches(&connect(None), &name));
        assert!(!name_matches(&connect(Some("MyHome2")), &name));
    }

    #[tokio::test]
    async fn tcp_reachable_true_for_listening_port() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        assert!(tcp_reachable(&addr).await);
    }

    #[tokio::test]
    async fn tcp_reachable_false_for_closed_port() {
        // 先占端口再立即释放，确保回环上该端口无人监听（避免 flaky 的固定端口假设）
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        drop(listener);
        assert!(!tcp_reachable(&addr).await);
    }

    fn task_with_network_trigger(
        enabled: bool,
        trigger_enabled: bool,
        events: Vec<NetworkEventKind>,
        network_name: Option<String>,
    ) -> Task {
        let task_id = TaskId::new();
        Task {
            id: task_id,
            name: "network test".to_string(),
            description: None,
            enabled,
            triggers: vec![Trigger {
                id: TriggerId::new(),
                task_id,
                enabled: trigger_enabled,
                kind: TriggerKind::Network {
                    events,
                    network_name,
                },
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            }],
            actions: vec![Action {
                id: ActionId::new(),
                task_id,
                sequence: 1,
                enabled: true,
                kind: ActionKind::ExecuteShell {
                    command: "true".to_string(),
                },
            }],
            execution_policy: ExecutionPolicy::default(),
            working_directory: None,
            environment: HashMap::new(),
            version: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn dispatch_triggers_only_matching_enabled_tasks() {
        let pool = init_pool("sqlite::memory:?cache=shared").await.unwrap();
        let repo = Arc::new(SqliteTaskRepository::new(pool));

        let matching = task_with_network_trigger(
            true,
            true,
            vec![NetworkEventKind::Connect],
            Some("Home".to_string()),
        );
        let matching_id = matching.id;

        // 事件类型匹配但 SSID 不匹配
        let ssid_mismatch = task_with_network_trigger(
            true,
            true,
            vec![NetworkEventKind::Connect],
            Some("Office".to_string()),
        );
        // 触发器被禁用
        let disabled_trigger =
            task_with_network_trigger(true, false, vec![NetworkEventKind::Connect], None);
        // 任务本身被禁用（find_all_enabled 不会返回）
        let disabled_task =
            task_with_network_trigger(false, true, vec![NetworkEventKind::Connect], None);

        for task in [&matching, &ssid_mismatch, &disabled_trigger, &disabled_task] {
            repo.save(task).await.unwrap();
        }

        let (tx, mut rx) = mpsc::channel(8);
        dispatch_network_event(
            NetworkEvent::Connect {
                ssid: Some("Home".to_string()),
            },
            repo,
            tx,
        )
        .await;

        match rx.try_recv().expect("one TriggerNow dispatched") {
            SchedulerCommand::TriggerNow(id) => assert_eq!(id, matching_id),
            other => panic!("expected TriggerNow, got {other:?}"),
        }
        assert!(rx.try_recv().is_err(), "only one task should be dispatched");
    }
}
