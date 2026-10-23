//! 用本机回环 HTTP 服务验证两个实现类的真实请求路径、鉴权头与解析结果。
//! 只在开发机上运行，不依赖外部服务。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

use token_refill_lib::quota::newapi::NewApiProvider;
use token_refill_lib::quota::sub2api::Sub2ApiProvider;
use token_refill_lib::quota::{detect_and_fetch, KeyQuotaProvider, QuotaMode, QuotaProvider};

const KEY: &str = "sk-test-key";

fn serve(responses: Vec<(u16, &'static str)>) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("绑定回环端口");
    let address = listener.local_addr().expect("读取本地地址");
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        for (status, body) in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buffer = [0u8; 8192];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let _ = sender.send(request);

            let reason = match status {
                200 => "OK",
                401 => "Unauthorized",
                _ => "Error",
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    (format!("http://127.0.0.1:{}", address.port()), receiver)
}

#[test]
fn newapi_reads_quota_per_unit_and_authenticates_the_usage_request() {
    let usage = r#"{
        "code": true,
        "message": "ok",
        "data": {
            "object": "token_usage",
            "name": "Default Token",
            "total_granted": 2000000,
            "total_used": 400000,
            "total_available": 1600000,
            "unlimited_quota": false,
            "expires_at": 0
        }
    }"#;
    let status = r#"{"success":true,"data":{"quota_per_unit":1000000}}"#;
    let (base, requests) = serve(vec![(200, usage), (200, status)]);

    let quota = NewApiProvider::new(&base, KEY)
        .expect("构造 New API 客户端")
        .fetch_quota()
        .expect("查询 New API 额度");

    let usage_request = requests.recv().expect("收到额度请求");
    let status_request = requests.recv().expect("收到状态请求");
    assert!(status_request.starts_with("GET /api/status "));
    assert!(usage_request.starts_with("GET /api/usage/token/ "));
    assert!(
        usage_request
            .to_lowercase()
            .contains("authorization: bearer sk-test-key"),
        "额度请求必须携带密钥：{usage_request}"
    );
    assert!(!status_request.to_lowercase().contains("authorization:"));

    assert_eq!(quota.provider, QuotaProvider::NewApi);
    assert_eq!(quota.mode, QuotaMode::Key);
    assert_eq!(quota.key_name.as_deref(), Some("Default Token"));
    assert_eq!(quota.limit_usd, Some(2.0));
    assert_eq!(quota.used_usd, Some(0.4));
    assert_eq!(quota.remaining_usd, Some(1.6));
    assert_eq!(quota.expires_at_epoch, None);
}

#[test]
fn sub2api_reads_key_quota_over_v1_usage() {
    let body = r#"{
        "mode": "quota_limited",
        "isValid": true,
        "quota": {"limit": 10, "used": 2.5, "remaining": 7.5, "unit": "USD"},
        "remaining": 7.5,
        "rate_limits": [
            {"window": "5h", "limit": 2, "used": 0.5, "remaining": 1.5, "reset_at": "2026-10-23T08:00:00Z"}
        ]
    }"#;
    let (base, requests) = serve(vec![(200, body)]);

    let quota = Sub2ApiProvider::new(&format!("{base}/v1"), KEY)
        .expect("构造 Sub2API 客户端")
        .fetch_quota()
        .expect("查询 Sub2API 额度");

    let request = requests.recv().expect("收到额度请求");
    assert!(request.starts_with("GET /v1/usage "));
    assert!(request
        .to_lowercase()
        .contains("authorization: bearer sk-test-key"));

    assert_eq!(quota.provider, QuotaProvider::Sub2Api);
    assert_eq!(quota.mode, QuotaMode::Key);
    assert_eq!(quota.limit_usd, Some(10.0));
    assert_eq!(quota.used_usd, Some(2.5));
    assert_eq!(quota.remaining_usd, Some(7.5));
    assert_eq!(quota.windows.len(), 1);
    assert_eq!(quota.windows[0].remaining_percent, Some(75));
}

#[test]
fn rejection_never_echoes_the_api_key() {
    let body = r#"{"success":false,"message":"invalid key sk-test-key"}"#;
    let (base, _requests) = serve(vec![(401, body)]);

    let error = Sub2ApiProvider::new(&base, KEY)
        .expect("构造 Sub2API 客户端")
        .fetch_quota()
        .expect_err("401 必须失败");

    assert!(error.contains("密钥无效"));
    assert!(!error.contains(KEY));
}

#[test]
fn base_url_requires_https_except_for_loopback() {
    assert!(NewApiProvider::new("https://api.example.com", KEY).is_ok());
    assert!(NewApiProvider::new("https://api.example.com/", KEY).is_ok());
    assert!(Sub2ApiProvider::new("http://127.0.0.1:8080", KEY).is_ok());
    assert!(Sub2ApiProvider::new("http://localhost:8080", KEY).is_ok());

    assert!(NewApiProvider::new("http://api.example.com", KEY).is_err());
    assert!(NewApiProvider::new("ftp://api.example.com", KEY).is_err());
    assert!(NewApiProvider::new("https://user:pass@api.example.com", KEY).is_err());
    assert!(NewApiProvider::new("https://api.example.com?token=1", KEY).is_err());
}

#[test]
fn empty_or_malformed_keys_are_rejected_before_any_request() {
    assert!(NewApiProvider::new("https://api.example.com", "  ").is_err());
    assert!(NewApiProvider::new("https://api.example.com", "sk-a b").is_err());
    assert!(NewApiProvider::new("https://api.example.com", "sk-a\nb").is_err());
}

#[test]
fn detection_falls_back_to_sub2api_when_newapi_is_absent() {
    let body = r#"{"mode":"quota_limited","isValid":true,
        "quota":{"limit":5,"used":1,"remaining":4}}"#;
    let (base, requests) = serve(vec![(404, "{}"), (200, body)]);

    let detected = detect_and_fetch(&base, KEY).expect("识别 Sub2API");

    assert_eq!(detected.provider, QuotaProvider::Sub2Api);
    assert_eq!(detected.quota.mode, QuotaMode::Key);
    assert_eq!(detected.quota.remaining_usd, Some(4.0));
    assert!(requests
        .recv()
        .unwrap()
        .starts_with("GET /api/usage/token/ "));
    assert!(requests.recv().unwrap().starts_with("GET /v1/usage "));
}

#[test]
fn detection_recognizes_newapi_first() {
    let usage = r#"{"code":true,"data":{"name":"中转","total_granted":1000000,
        "total_used":0,"total_available":1000000,"unlimited_quota":false}}"#;
    let status = r#"{"success":true,"data":{"quota_per_unit":500000}}"#;
    let (base, requests) = serve(vec![(200, usage), (200, status)]);

    let detected = detect_and_fetch(&base, KEY).expect("识别 New API");

    assert_eq!(detected.provider, QuotaProvider::NewApi);
    assert_eq!(detected.quota.limit_usd, Some(2.0));
    assert!(requests
        .recv()
        .unwrap()
        .starts_with("GET /api/usage/token/ "));
    assert!(requests.recv().unwrap().starts_with("GET /api/status "));
}

#[test]
fn detection_error_lists_both_attempts_without_the_key() {
    let (base, _requests) = serve(vec![(401, "{}"), (404, "{}")]);

    let error = detect_and_fetch(&base, KEY).expect_err("两种接口都失败");

    assert!(error.contains("New API"));
    assert!(error.contains("Sub2API"));
    assert!(!error.contains(KEY));
}
