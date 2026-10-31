//! macOS 系统通知：`UNUserNotificationCenter` 的最小封装。
//!
//! 只做三件事：申请授权、注册前台展示代理、发送本地通知。前台代理请求
//! `banner`，实际横幅展示仍由系统通知设置、前台抑制与专注模式决定。

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
            // 请求前台展示、声音和通知中心记录，不把展示请求视为送达回执。
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
                Ok(status) if status == UNAuthorizationStatus::NotDetermined => {
                    match request_authorization(&center) {
                        Ok(true) => {}
                        Ok(false) => eprintln!("系统通知授权被拒绝"),
                        Err(message) => eprintln!("{message}"),
                    }
                }
                Ok(status) if status == UNAuthorizationStatus::Denied => {
                    eprintln!("系统通知权限已被拒绝，请在系统设置中允许");
                }
                Err(message) => eprintln!("{message}"),
                _ => {}
            }
        });
    if let Err(err) = spawned {
        eprintln!("启动通知授权线程失败：{err}");
    }
}

pub fn send(app_name: &str, title: &str, body: &str) -> Result<(), String> {
    send_with_identifier(app_name, title, body, &notification_identifier())
}

pub fn send_with_identifier(
    app_name: &str,
    title: &str,
    body: &str,
    identifier: &str,
) -> Result<(), String> {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    install_delegate(&center);

    // 查询失败时尚未提交通知，可以安全地留给调用方稍后重试。
    match authorization_status(&center)? {
        status if status == UNAuthorizationStatus::Denied => Err(format!(
            "系统通知权限已被拒绝，请在“系统设置 → 通知 → {app_name}”中允许。"
        )),
        status if status == UNAuthorizationStatus::NotDetermined => {
            // 启动时由 prepare 申请权限；重试只等待决定，避免重复启动授权线程。
            Err(
                "尚未允许系统通知；若未出现权限弹窗，请退出并重新打开应用，或检查系统通知设置。"
                    .to_string(),
            )
        }
        _ => deliver(&center, title, body, identifier),
    }
}

fn deliver(
    center: &UNUserNotificationCenter,
    title: &str,
    body: &str,
    identifier: &str,
) -> Result<(), String> {
    // SAFETY: `new` 按 UNMutableNotificationContent 的初始化契约返回有效实例，
    // 之后的 setter 都在该实例存活期间调用。
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));

    // 同次提醒重试复用标识，替换仍待投递的请求；已投递的通知仍可能再次展示。
    let identifier = NSString::from_str(identifier);
    let request =
        UNNotificationRequest::requestWithIdentifier_content_trigger(&identifier, &content, None);

    let (tx, rx) = mpsc::channel();
    let handler = RcBlock::new(move |error: *mut NSError| {
        // 只传递成功标记，不把系统错误可能携带的通知内容暴露给调用方。
        let _ = tx.send(error.is_null());
    });
    center.addNotificationRequest_withCompletionHandler(&request, Some(&handler));

    delivery_result(rx.recv_timeout(Duration::from_secs(3)))
}

fn delivery_result(result: Result<bool, mpsc::RecvTimeoutError>) -> Result<(), String> {
    match result {
        Ok(true) => Ok(()),
        Ok(false) => Err("发送通知失败：系统未接受通知请求。".to_string()),
        // 请求已提交；超时或断连不代表系统未发送，立即重试可能造成重复提醒。
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err("通知提交结果未知：等待系统确认超时，通知可能已发送。".to_string())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("通知提交结果未知：系统确认通道已断开，通知可能已发送。".to_string())
        }
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

fn authorization_status(
    center: &UNUserNotificationCenter,
) -> Result<UNAuthorizationStatus, String> {
    let (tx, rx) = mpsc::channel();
    let handler = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
        // SAFETY: 回调期间系统保证 settings 有效。
        let status = unsafe { settings.as_ref() }.authorizationStatus();
        let _ = tx.send(status);
    });
    center.getNotificationSettingsWithCompletionHandler(&handler);
    authorization_status_result(rx.recv_timeout(Duration::from_secs(2)))
}

fn authorization_status_result(
    result: Result<UNAuthorizationStatus, mpsc::RecvTimeoutError>,
) -> Result<UNAuthorizationStatus, String> {
    result.map_err(|_| "读取系统通知权限失败，尚未发送通知，请稍后重试。".to_string())
}

type AuthorizationReply = Result<bool, (String, isize)>;

fn request_authorization(center: &UNUserNotificationCenter) -> Result<bool, String> {
    let (tx, rx) = mpsc::channel();
    let handler = RcBlock::new(move |granted: Bool, error: *mut NSError| {
        // SAFETY: 系统在回调期间保证非空 NSError 指针有效；只复制错误域和码，
        // 不读取 userInfo 或 localizedDescription，避免输出可能携带的上下文。
        let result = match unsafe { error.as_ref() } {
            Some(error) => Err((error.domain().to_string(), error.code())),
            None => Ok(granted.as_bool()),
        };
        let _ = tx.send(result);
    });
    center.requestAuthorizationWithOptions_completionHandler(authorization_options(), &handler);
    authorization_request_result(rx.recv_timeout(Duration::from_secs(120)))
}

fn authorization_request_result(
    result: Result<AuthorizationReply, mpsc::RecvTimeoutError>,
) -> Result<bool, String> {
    match result {
        Ok(Ok(granted)) => Ok(granted),
        Ok(Err((domain, code))) => Err(format!(
            "申请系统通知权限失败（错误域：{domain}，错误码：{code}）；这不代表用户拒绝授权。"
        )),
        Err(_) => Err("系统通知权限申请结果未知，请检查系统通知设置后重试。".to_string()),
    }
}

fn notification_identifier() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("token-refill-{nanos}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_requires_successful_completion() {
        assert_eq!(delivery_result(Ok(true)), Ok(()));
        assert_eq!(
            delivery_result(Ok(false)).unwrap_err(),
            "发送通知失败：系统未接受通知请求。"
        );
    }

    #[test]
    fn delivery_timeout_is_unknown_not_success() {
        assert_eq!(
            delivery_result(Err(mpsc::RecvTimeoutError::Timeout)).unwrap_err(),
            "通知提交结果未知：等待系统确认超时，通知可能已发送。"
        );
    }

    #[test]
    fn delivery_disconnection_is_unknown_not_success() {
        assert_eq!(
            delivery_result(Err(mpsc::RecvTimeoutError::Disconnected)).unwrap_err(),
            "通知提交结果未知：系统确认通道已断开，通知可能已发送。"
        );
    }

    #[test]
    fn authorization_request_distinguishes_system_failure_from_user_denial() {
        assert_eq!(authorization_request_result(Ok(Ok(true))), Ok(true));
        assert_eq!(authorization_request_result(Ok(Ok(false))), Ok(false));
        let error = authorization_request_result(Ok(Err(("UNErrorDomain".to_string(), 1))))
            .expect_err("system error must not be treated as a user decision");
        assert!(error.contains("UNErrorDomain"));
        assert!(error.contains("错误码：1"));
        for error in [
            mpsc::RecvTimeoutError::Timeout,
            mpsc::RecvTimeoutError::Disconnected,
        ] {
            assert!(authorization_request_result(Err(error)).is_err());
        }
    }

    #[test]
    fn authorization_requires_a_confirmed_status() {
        for status in [
            UNAuthorizationStatus::NotDetermined,
            UNAuthorizationStatus::Denied,
            UNAuthorizationStatus::Authorized,
            UNAuthorizationStatus::Provisional,
        ] {
            assert_eq!(authorization_status_result(Ok(status)), Ok(status));
        }
        for error in [
            mpsc::RecvTimeoutError::Timeout,
            mpsc::RecvTimeoutError::Disconnected,
        ] {
            assert_eq!(
                authorization_status_result(Err(error)).unwrap_err(),
                "读取系统通知权限失败，尚未发送通知，请稍后重试。"
            );
        }
    }
}
