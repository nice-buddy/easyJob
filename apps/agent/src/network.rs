use std::sync::Arc;
use std::time::Duration;

use easyjob_domain::trigger::{NetworkEventKind, TriggerKind};
use easyjob_persistence::task_repo::{SqliteTaskRepository, TaskRepository};
use easyjob_scheduler::scheduler::SchedulerCommand;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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

/// 是否为公网地址：内网 DNS 可能能解析出域名但整机仍出不了外网，
/// 因此解析结果里只认公网地址。
fn is_public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_unspecified()
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                // 100.64.0.0/10 运营商级 NAT
                || (o[0] == 100 && (64..128).contains(&o[1]))
                // 0.0.0.0/8
                || o[0] == 0)
        }
        std::net::IpAddr::V6(v6) => {
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                // fe80::/10 链路本地
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                // fc00::/7 唯一本地地址
                || (v6.segments()[0] & 0xfe00) == 0xfc00)
        }
    }
}

/// 联网探测端点：(域名, 路径, 判定标记)。
/// 标记为 None 表示要求 HTTP 204；Some(marker) 表示要求 HTTP 200 且响应体含该标记。
/// 第一个是 Windows NCSI 用的探测点，第二个为国内可用的探测点，第三个是国际兜底。
const ONLINE_PROBES: &[(&str, &str, Option<&str>)] = &[
    (
        "www.msftconnecttest.com",
        "/connecttest.txt",
        Some("Microsoft Connect Test"),
    ),
    ("connect.rom.miui.com", "/generate_204", None),
    ("cp.cloudflare.com", "/generate_204", None),
];

const PROBE_DNS_TIMEOUT: Duration = Duration::from_millis(1500);
const PROBE_CONNECT_TIMEOUT: Duration = Duration::from_millis(1500);
const PROBE_READ_TIMEOUT: Duration = Duration::from_millis(1500);

/// 校验一次探测响应是否符合预期。抽成纯函数，便于不依赖网络的单测。
fn response_matches(raw: &[u8], marker: Option<&str>) -> bool {
    let text = String::from_utf8_lossy(raw);
    let Some(status) = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
    else {
        return false;
    };
    match marker {
        Some(m) => status == "200" && text.contains(m),
        None => status == "204",
    }
}

/// 单端点探测：解析域名 → 连 80 → 发最小 GET → 按预期校验响应。
/// 只认公网解析结果；必须拿到预期的应用层响应，TCP 能连上不算数。
async fn http_probe(host: &str, path: &str, marker: Option<&str>) -> bool {
    let Ok(Ok(addrs)) = tokio::time::timeout(
        PROBE_DNS_TIMEOUT,
        tokio::net::lookup_host((host, 80)),
    )
    .await
    else {
        return false;
    };

    // IPv4 优先，避免在只有 IPv6 被黑洞的网络里白等一轮
    let mut addrs: Vec<std::net::SocketAddr> = addrs
        .filter(|sa| is_public_ip(sa.ip()))
        .collect();
    addrs.sort_by_key(|sa| sa.is_ipv6());

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: easyJob\r\nConnection: close\r\n\r\n"
    );

    for addr in addrs.into_iter().take(2) {
        let Ok(Ok(mut stream)) =
            tokio::time::timeout(PROBE_CONNECT_TIMEOUT, tokio::net::TcpStream::connect(addr)).await
        else {
            continue;
        };
        if tokio::time::timeout(PROBE_READ_TIMEOUT, stream.write_all(request.as_bytes()))
            .await
            .map(|r| r.is_err())
            .unwrap_or(true)
        {
            continue;
        }

        let mut raw = Vec::new();
        let _ = tokio::time::timeout(PROBE_READ_TIMEOUT, async {
            let mut chunk = [0u8; 1024];
            while raw.len() < 8192 {
                match stream.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => raw.extend_from_slice(&chunk[..n]),
                }
            }
        })
        .await;

        if response_matches(&raw, marker) {
            return true;
        }
    }
    false
}

/// 共享 Online 探测：必须拿到真实的应用层响应才算在线。
/// 内网 DNS 常把公网域名转发解析成功（能解析 ≠ 能上网），透明代理也可能完成
/// TCP 握手，因此只有解析 + 建连 + 预期响应全部成立才判为在线。
pub async fn probe_online() -> bool {
    for (host, path, marker) in ONLINE_PROBES {
        if http_probe(host, path, *marker).await {
            tracing::debug!("online probe ok: {host}{path}");
            return true;
        }
    }
    false
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

/// 状态边沿检测：把「状态快照回调」收敛成「只在状态真正变化时才上报」。
///
/// 平台 API 的通知常常在状态**未变**时也会回调（path 因路由/DNS/接口增删等变化、
/// 接口参数变更等）。若按回调逐帧上报，就会把同一状态的重复快照当成新事件发出。
/// 这里按 key 记录最近一次观测到的值：
/// - 首次见到该 key：只记录、不上报（启动时不产生事件）
/// - 与上次观测相同：不上报
/// - 与上次观测不同：上报
pub(crate) struct EdgeDetector<K> {
    seen: std::sync::Mutex<std::collections::HashMap<K, bool>>,
}

impl<K: std::hash::Hash + Eq + Clone> EdgeDetector<K> {
    pub fn new() -> Self {
        Self {
            seen: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// 返回 true 表示「该 key 的状态相对上次观测发生了变化」，调用方应当上报事件。
    pub fn changed(&self, key: K, value: bool) -> bool {
        let mut seen = self.seen.lock().unwrap();
        match seen.insert(key, value) {
            None => false,
            Some(previous) => previous != value,
        }
    }
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
    use super::{Debouncer, EdgeDetector, NetworkEvent};
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
            // 只在 path 的 status 真正发生变化时上报。
            //
            // nw_path_monitor 的 update handler 是在 path 发生**任意**变化时被调用的
            // （接口增删、路由、DNS 等），并非只在 status 变化时调用；接口切换过程中它还会
            // 先给出一帧仍带**旧** status 的回调。若按回调逐帧上报，切换 Wi-Fi 时会先发出
            // 一个方向相反的事件（实测：断开时先 Connect 再 Disconnect、连接时先
            // Disconnect 再 Connect），把对侧任务误触发。
            //
            // 用 EdgeDetector 同时取代了原先「跳过 start 首帧」的标记：首帧只记录状态、
            // 不上报，与 Windows 侧 InitialNotification=false 的意图一致，且不依赖
            // 「start 恰好只投递一帧」这一假设。
            let cb_edge = Arc::new(EdgeDetector::new());
            let cb = RcBlock::new(move |path: *mut c_void| {
                if !cb_running.load(Ordering::Relaxed) {
                    return;
                }
                let status = unsafe { nw_path_get_status(path) };
                let Some(connected) = status_to_connect_change(status, &cb_edge) else {
                    return;
                };
                let event = if connected {
                    NetworkEvent::Connect {
                        ssid: current_ssid(),
                    }
                } else {
                    NetworkEvent::Disconnect {
                        ssid: current_ssid(),
                    }
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

    /// 把一次 path 回调的原始 status 映射为「是否已连接」，并做状态边沿检测。
    ///
    /// 返回 `Some(connected)` 表示该 status 相对上次观测**发生了变化**、应当上报；
    /// 返回 `None` 表示不上报（首帧、重复快照、或 Satisfiable/Invalid 等中间态）。
    ///
    /// macOS 只有一个默认 path，故 EdgeDetector 的 key 固定为 0。
    fn status_to_connect_change(status: i32, edge: &EdgeDetector<u8>) -> Option<bool> {
        match status {
            NW_PATH_STATUS_SATISFIED => edge.changed(0, true).then_some(true),
            NW_PATH_STATUS_UNSATISFIED => edge.changed(0, false).then_some(false),
            _ => None,
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

    #[cfg(test)]
    mod tests {
        use super::*;

        /// 复刻实测日志（2026-09-19 02:40Z）里一次切换的原始 status 序列：
        /// - 断开 Wi-Fi：[1(旧), 2(新), 2]  ← 首帧 1 来自启动时的快照
        /// - 开 Wi-Fi：  [2(旧), 1(新), 1, 1]
        ///
        /// 期望：每次切换只上报**一个方向正确**的事件，不出现相反事件。
        #[test]
        fn transition_reports_only_the_new_status() {
            let edge = EdgeDetector::new();
            let mut reported = Vec::new();
            for status in [1, 1, 2, 2, 1, 1, 1] {
                if let Some(connected) = status_to_connect_change(status, &edge) {
                    reported.push(connected);
                }
            }
            // 只有「断开」与「重新连接」两次真实变化；不出现 1→2→1 的假事件
            assert_eq!(reported, vec![false, true]);
        }

        #[test]
        fn middle_states_and_repeats_do_not_report() {
            let edge = EdgeDetector::new();
            assert_eq!(status_to_connect_change(1, &edge), None); // 首帧只记录
            assert_eq!(status_to_connect_change(3, &edge), None); // satisfiable 中间态
            assert_eq!(status_to_connect_change(0, &edge), None); // invalid 中间态
            assert_eq!(status_to_connect_change(1, &edge), None); // 状态未变
            assert_eq!(status_to_connect_change(2, &edge), Some(false));
            assert_eq!(status_to_connect_change(2, &edge), None); // 状态未变
            assert_eq!(status_to_connect_change(1, &edge), Some(true));
        }
    }
}

#[cfg(target_os = "windows")]
mod platform_windows {
    use super::{Debouncer, EdgeDetector, NetworkEvent};
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicI8, AtomicI64, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
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
        // 按接口 LUID 记录上一次观测到的连通性，避免重复/无关通知被误报成事件
        edge: EdgeDetector<u64>,
        // 回调在系统线程触发，那里没有 Tokio 上下文，故在 start() 内取 Handle
        probe_handle: Option<tokio::runtime::Handle>,
        // 整机联网状态：-1 未知（尚未建立基线）、0 离线、1 在线。
        // 事件只在「整机能不能出外网」发生翻转时才上报，与是哪块网卡抖动无关。
        online_state: Arc<AtomicI8>,
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
                edge: EdgeDetector::new(),
                probe_handle: tokio::runtime::Handle::try_current().ok(),
                online_state: Arc::new(AtomicI8::new(-1)),
            });

            // 1) IP 接口变动（有线 + 通用 up/down）
            start_interface_watch(state.clone());

            // 2) WLAN 专用通知（提供 SSID）
            start_wlan_watch(state.clone());

            // 3) 建立联网状态基线：启动时只记录状态，不补报事件，
            //    否则应用一启动就会凭空报一次「连接网络」。
            if let Some(handle) = state.probe_handle.clone() {
                let baseline = state.clone();
                handle.spawn(async move {
                    tokio::time::sleep(BASELINE_SETTLE).await;
                    let online = confirm_online().await;
                    let value = if online { 1 } else { 0 };
                    let _ = baseline
                        .online_state
                        .compare_exchange(-1, value, Ordering::Relaxed, Ordering::Relaxed);
                });
            }

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
        //
        // 通知是在接口**任意**参数变化（MTU / metric / DNS / 地址）时到达的，
        // 并非只在连通性变化时到达。禁用/启用适配器的 teardown/setup 过程中会先
        // 产生一串中间态通知，直接按单接口 Connected 定方向会把事件报反
        //（禁用报 Connect、启用报 Disconnect）。
        //
        // 因此这里只做边沿去重（同一接口重复通知合并），方向由整机连通性仲裁决定：
        // 等待接口状态稳定后探测整机可达性，通 → Connect，不通 → Disconnect。
        // MibDeleteInstance（接口行被移除）同样走仲裁（此时多半已不通）。
        let changed = if row.is_null() {
            // 拿不到接口行（例如接口已被移除）时无法按 LUID 去重，
            // 直接交给整机状态机判断，避免漏掉「断开」这类关键边沿。
            true
        } else {
            let key = unsafe { (*row).InterfaceLuid.Value };
            let connected = notification_type != MibDeleteInstance && unsafe { (*row).Connected };
            state.edge.changed(key, connected)
        };
        if !changed {
            return;
        }
        spawn_arbitrated_event(&state);
    }

    /// 边沿稳定期：等适配器 teardown / 链路建立结束
    const EDGE_SETTLE: Duration = Duration::from_millis(1000);
    /// 启动时建立联网基线的延迟：等网络栈初始化完成再判定
    const BASELINE_SETTLE: Duration = Duration::from_millis(2000);
    /// 联网确认窗口：接口起来后 DHCP / 路由就绪可能晚于一次探测
    const ONLINE_CONFIRM_WINDOW: Duration = Duration::from_secs(7);
    /// 窗口内单次探测的耗时上限
    const PROBE_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(3);

    /// 网络边沿后的整机连通性仲裁：接口状态稳定后，在一个窗口内反复确认能否
    /// 真正出外网，再与上一次已知状态比较，只有整机联网状态翻转才上报事件。
    ///
    /// 这样「禁用外网网卡」只会报断开、「启用」只会报连接 + 能上网，
    /// 而另一块网卡或 WiFi 的抖动不会凭空造出事件。
    fn spawn_arbitrated_event(state: &WatcherState) {
        static ARBITRATE_SEQ: AtomicI64 = AtomicI64::new(0);
        let seq = ARBITRATE_SEQ.fetch_add(1, Ordering::Relaxed) + 1;

        let on_event = state.on_event.clone();
        let debouncer = state.debouncer.clone();
        let online_state = state.online_state.clone();
        let Some(handle) = state.probe_handle.clone() else {
            return;
        };
        handle.spawn(async move {
            tokio::time::sleep(EDGE_SETTLE).await;
            if ARBITRATE_SEQ.load(Ordering::Relaxed) != seq {
                return;
            }
            let reachable = confirm_online().await;
            // 只有最新一次边沿才真正上报，中间的抖动直接丢弃
            if ARBITRATE_SEQ.load(Ordering::Relaxed) != seq {
                return;
            }
            let current: i8 = if reachable { 1 } else { 0 };
            let previous = online_state.load(Ordering::Relaxed);
            if previous == current {
                // 整机联网状态没变（例如另一块网卡抖动），不重复上报
                return;
            }
            online_state.store(current, Ordering::Relaxed);
            info!("machine online state: {previous} -> {current}");
            if reachable {
                emit_arbitrated(
                    &on_event,
                    &debouncer,
                    NetworkEvent::Connect {
                        ssid: query_current_ssid(),
                    },
                );
                emit_arbitrated(
                    &on_event,
                    &debouncer,
                    NetworkEvent::Online {
                        ssid: query_current_ssid(),
                    },
                );
            } else {
                emit_arbitrated(&on_event, &debouncer, NetworkEvent::Disconnect { ssid: None });
            }
        });
    }

    /// 窗口内反复确认联网状态：任一次确认在线即返回 true；窗口耗尽仍未确认才算离线。
    /// 启用网卡时 DHCP / 路由就绪慢一点，也不会被误判成断开。
    async fn confirm_online() -> bool {
        let start = std::time::Instant::now();
        loop {
            if tokio::time::timeout(PROBE_ATTEMPT_TIMEOUT, super::probe_online())
                .await
                .unwrap_or(false)
            {
                return true;
            }
            if start.elapsed() + Duration::from_millis(500) >= ONLINE_CONFIRM_WINDOW {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    fn emit_arbitrated(on_event: &SendFn, debouncer: &Debouncer, event: NetworkEvent) {
        if !debouncer.allow(event.kind()) {
            return;
        }
        info!("network event (arbitrated): {:?}", event);
        on_event(event);
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
        // 只关心「关联完成 / 已断开」；其余 ACM 通知（扫描、认证中间态等）忽略。
        // 方向不按 WLAN 通知本身判定：WiFi 掉线时整机可能仍能出外网（有线还在），
        // 反之 WiFi 连上也可能没有外网，统一交给整机联网状态机决定。
        let relevant = n.NotificationCode == wlan_notification_acm_connection_complete.0 as u32
            || n.NotificationCode == wlan_notification_acm_disconnected.0 as u32;
        if !relevant {
            return;
        }
        spawn_arbitrated_event(state);
    }

    impl WatcherState {
        fn running_flag(&self) -> bool {
            self.running.load(Ordering::Relaxed)
        }
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
    fn edge_detector_reports_only_real_changes() {
        let edge = EdgeDetector::new();
        // 首帧只记录，不上报（启动时不产生事件）
        assert!(!edge.changed(0u8, true));
        // 同一状态的回调：不上报。这正是切换 Wi-Fi 时先发出「相反事件」的根因
        assert!(!edge.changed(0u8, true));
        // 真实变化才上报
        assert!(edge.changed(0u8, false));
        assert!(!edge.changed(0u8, false));
        assert!(edge.changed(0u8, true));
        assert!(!edge.changed(0u8, true));
    }

    #[test]
    fn edge_detector_tracks_keys_independently() {
        let edge: EdgeDetector<&str> = EdgeDetector::new();
        assert!(!edge.changed("eth0", true));
        assert!(!edge.changed("wlan0", true)); // 另一个接口首次出现：各自初始化
        assert!(edge.changed("eth0", false)); // 只影响自己的 key
        assert!(!edge.changed("wlan0", true));
        assert!(!edge.changed("eth0", false));
    }

    #[test]
    fn response_matches_only_accepts_expected_online_response() {
        // 204 约定：只认 204，200/302/空响应都不算在线
        assert!(response_matches(b"HTTP/1.1 204 No Content\r\n\r\n", None));
        assert!(!response_matches(b"HTTP/1.1 200 OK\r\n\r\n<html>", None));
        assert!(!response_matches(b"HTTP/1.1 302 Found\r\nLocation: /\r\n\r\n", None));
        assert!(!response_matches(b"", None));
        // 标记约定：必须 200 且响应体含标记（抓门户劫持返回的 HTML 页面）
        assert!(response_matches(
            b"HTTP/1.1 200 OK\r\n\r\nMicrosoft Connect Test\r\n",
            Some("Microsoft Connect Test")
        ));
        assert!(!response_matches(
            b"HTTP/1.1 200 OK\r\n\r\n<html>login</html>",
            Some("Microsoft Connect Test")
        ));
        assert!(!response_matches(
            b"HTTP/1.1 302 Found\r\n\r\nMicrosoft Connect Test",
            Some("Microsoft Connect Test")
        ));
    }

    #[test]
    fn is_public_ip_rejects_private_and_keeps_public() {
        use std::net::IpAddr;
        // 内网 / 保留地址：不算可上网
        for s in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.10.10",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fc00::1",
        ] {
            let ip: IpAddr = s.parse().unwrap();
            assert!(!is_public_ip(ip), "{s} 不应判为公网");
        }
        // 公网地址：算可上网
        for s in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
            let ip: IpAddr = s.parse().unwrap();
            assert!(is_public_ip(ip), "{s} 应判为公网");
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
