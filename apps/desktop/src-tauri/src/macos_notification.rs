#[cfg(target_os = "macos")]
pub mod macos {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
    use std::ffi::c_void;
    use std::sync::OnceLock;
    use tauri::{AppHandle, Emitter, Manager};
    use tracing::{info, warn};

    static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
    static ORIGINAL_DID_ACTIVATE: OnceLock<Imp> = OnceLock::new();

    extern "C-unwind" fn should_present(
        _this: *mut c_void,
        _cmd: Sel,
        _center: *mut c_void,
        _notif: *mut c_void,
    ) -> Bool {
        Bool::YES
    }

    extern "C-unwind" fn swizzled_did_activate(
        this: *mut c_void,
        cmd: Sel,
        center: *mut c_void,
        notif: *mut c_void,
    ) {
        info!("macOS notification clicked - activating window and navigating to executions");
        if let Some(app) = APP_HANDLE.get() {
            crate::set_dock_visible(true);
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
                let _ = window.emit("navigate", "executions");
            }
        }

        if let Some(orig_imp) = ORIGINAL_DID_ACTIVATE.get() {
            unsafe {
                let orig_fn: unsafe extern "C-unwind" fn(
                    *mut c_void,
                    Sel,
                    *mut c_void,
                    *mut c_void,
                ) = std::mem::transmute(*orig_imp);
                orig_fn(this, cmd, center, notif);
            }
        }
    }

    pub fn setup_macos_notifications(app: &AppHandle) {
        let _ = APP_HANDLE.set(app.clone());

        let _ = notify_rust::set_application("com.easyjob.desktop");

        if let Some(cls) = AnyClass::get(c"NotificationCenterDelegate") {
            unsafe {
                let delegate: *mut AnyObject = msg_send![cls, sharedDelegate];
                if let Some(center_cls) = AnyClass::get(c"NSUserNotificationCenter") {
                    let center: *mut AnyObject =
                        msg_send![center_cls, defaultUserNotificationCenter];
                    let _: () = msg_send![center, setDelegate: delegate];
                }
            }

            // Register shouldPresentNotification: returning YES so banner is presented even when foreground
            let sel_present = objc2::sel!(userNotificationCenter:shouldPresentNotification:);
            let imp_present: unsafe extern "C-unwind" fn() =
                unsafe { std::mem::transmute(should_present as *const ()) };
            let types_present = std::ffi::CString::new("c@:@@").unwrap();
            let added_present = unsafe {
                objc2::ffi::class_addMethod(
                    cls as *const AnyClass as *mut _,
                    sel_present,
                    imp_present,
                    types_present.as_ptr(),
                )
            };
            info!(
                "macOS foreground notification shouldPresentNotification registered: {}",
                added_present.as_bool()
            );

            // Swizzle didActivateNotification: to navigate directly to executions
            let sel_activate = objc2::sel!(userNotificationCenter:didActivateNotification:);
            if let Some(method) = cls.instance_method(sel_activate) {
                let new_imp: unsafe extern "C-unwind" fn() =
                    unsafe { std::mem::transmute(swizzled_did_activate as *const ()) };
                let old_imp = unsafe { method.set_implementation(new_imp) };
                let _ = ORIGINAL_DID_ACTIVATE.set(old_imp);
                info!("macOS notification didActivateNotification swizzled successfully");
            } else {
                warn!("Could not find userNotificationCenter:didActivateNotification: method");
            }
        } else {
            warn!("NotificationCenterDelegate class not found during macOS notification setup");
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub mod macos {
    use tauri::AppHandle;
    pub fn setup_macos_notifications(_app: &AppHandle) {}
}

pub use macos::setup_macos_notifications;
