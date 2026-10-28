//! macOS 系统通知：`UNUserNotificationCenter` 的最小封装。
//!
//! 只做三件事：申请授权、注册前台展示代理、发送本地通知。前台代理返回
//! `banner`，因此应用在前台时提醒同样会弹出横幅，而不是只落进通知中心。

use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::{define_class, msg_send, ClassType};
use objc2_foundation::{NSBundle, NSError, NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationSettings,
    UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TokenRefillNotificationDelegate"]
    struct NotificationDelegate;

    unsafe impl NSObjectProtocol for NotificationDelegate {}

    unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion_handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            // 应用在前台时也要弹横幅、响声音、进通知中心。
            completion_handler.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound
                | UNNotificationPresentationOptions::Badge,));
        }
    }
);

static DELEGATE_INSTALLED: AtomicBool = AtomicBool::new(false);

/// 未打包的开发运行没有 bundle identifier，`UNUserNotificationCenter` 无法工作；
/// 调用方应退回插件实现。
pub fn available() -> bool {
    NSBundle::mainBundle().bundleIdentifier().is_some()
}

/// 应用启动时提前申请授权；已决定过时系统不会再弹框。
pub fn prepare() {
    if !available() {
        return;
    }

    let spawned = std::thread::Builder::new()
        .name("notification-auth".into())
        .spawn(|| {
            let center = UNUserNotificationCenter::currentNotificationCenter();
            install_delegate(&center);

            match authorization_status(&center) {
                Some(status) if status == UNAuthorizationStatus::NotDetermined => {
                    if request_authorization(&center) == Some(false) {
                        eprintln!("系统通知授权被拒绝");
                    }
                }
                Some(status) if status == UNAuthorizationStatus::Denied => {
                    eprintln!("系统通知权限已被拒绝，请在系统设置中允许");
                }
                _ => {}
            }
        });
    if let Err(err) = spawned {
        eprintln!("启动通知授权线程失败：{err}");
    }
}

pub fn send(app_name: &str, title: &str, body: &str) -> Result<(), String> {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    install_delegate(&center);

    match authorization_status(&center) {
        Some(status) if status == UNAuthorizationStatus::Denied => Err(format!(
            "系统通知权限已被拒绝，请在“系统设置 → 通知 → {app_name}”中允许。"
        )),
        Some(status) if status == UNAuthorizationStatus::NotDetermined => {
            // 授权框需要用户点击；本次不发，避免提醒被授权流程吞掉。
            let _ = std::thread::Builder::new()
                .name("notification-auth".into())
                .spawn(|| {
                    let center = UNUserNotificationCenter::currentNotificationCenter();
                    let _ = request_authorization(&center);
                });
            Err("需要先允许系统通知：请在系统弹窗中点击“允许”后重试。".to_string())
        }
        _ => deliver(&center, title, body),
    }
}

fn deliver(center: &UNUserNotificationCenter, title: &str, body: &str) -> Result<(), String> {
    // SAFETY: `new` 按 UNMutableNotificationContent 的初始化契约返回有效实例，
    // 之后的 setter 都在该实例存活期间调用。
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));

    let identifier = NSString::from_str(&notification_identifier());
    let request =
        UNNotificationRequest::requestWithIdentifier_content_trigger(&identifier, &content, None);

    let (tx, rx) = mpsc::channel();
    let handler = RcBlock::new(move |error: *mut NSError| {
        // SAFETY: 系统只在失败时传入非空指针，且回调期间对象有效。
        let message =
            unsafe { error.as_ref() }.map(|error| error.localizedDescription().to_string());
        let _ = tx.send(message);
    });
    center.addNotificationRequest_withCompletionHandler(&request, Some(&handler));

    match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Some(message)) => Err(format!("发送通知失败：{message}")),
        _ => Ok(()),
    }
}

fn install_delegate(center: &UNUserNotificationCenter) {
    if DELEGATE_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }

    // SAFETY: 委托对象只在进程内使用；center 的 delegate 是弱引用，
    // 所以对象泄漏到进程结束，避免被提前释放。
    unsafe {
        let delegate: Retained<NotificationDelegate> =
            msg_send![NotificationDelegate::class(), new];
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        std::mem::forget(delegate);
    }
}

fn authorization_options() -> UNAuthorizationOptions {
    UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound | UNAuthorizationOptions::Badge
}

fn authorization_status(center: &UNUserNotificationCenter) -> Option<UNAuthorizationStatus> {
    let (tx, rx) = mpsc::channel();
    let handler = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
        // SAFETY: 回调期间系统保证 settings 有效。
        let status = unsafe { settings.as_ref() }.authorizationStatus();
        let _ = tx.send(status);
    });
    center.getNotificationSettingsWithCompletionHandler(&handler);
    rx.recv_timeout(Duration::from_secs(2)).ok()
}

fn request_authorization(center: &UNUserNotificationCenter) -> Option<bool> {
    let (tx, rx) = mpsc::channel();
    let handler = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        let _ = tx.send(granted.as_bool());
    });
    center.requestAuthorizationWithOptions_completionHandler(authorization_options(), &handler);
    // 留给用户操作授权框；超时只是拿不到结果，不影响后续发送尝试。
    rx.recv_timeout(Duration::from_secs(120)).ok()
}

fn notification_identifier() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("token-refill-{nanos}")
}
