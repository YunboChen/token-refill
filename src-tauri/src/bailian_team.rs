//! 阿里云百炼 Token Plan 团队版（企业版）额度只读适配器。
//!
//! 团队版的额度不在控制台会话里：控制台页面走的是浏览器 Cookie、`sec_token`
//! 和 `collina` 风控指纹，CLI 回环授权拿到的控制台令牌会被拒绝
//! （`ConsoleNeedLogin`，已实测）。官方只提供 AccessKey 签名的 OpenAPI：
//!
//! - `GET https://modelstudio.cn-beijing.aliyuncs.com/tokenplan/subscription/stats`
//! - 签名算法 `ACS3-HMAC-SHA256`，请求头带 `x-acs-action` / `x-acs-version` 等
//!
//! 这里只实现一个只读统计接口。AccessKey 只写入应用私有目录，不进入前端状态、
//! 事件或日志；凭证文件与状态文件分开，权限沿用控制台凭证的 0700/0600。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::bailian::BailianError;

const API_HOST: &str = "modelstudio.cn-beijing.aliyuncs.com";
const STATS_PATH: &str = "/tokenplan/subscription/stats";
const STATS_ACTION: &str = "GetSubscriptionStats";
const ACS_VERSION: &str = "2026-02-10";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const CREDENTIALS_FILE: &str = "credentials.json";
/// AccessKey ID 通常以 `LTAI` 开头且长度固定；这里只做宽松校验，避免误伤新格式。
const ACCESS_KEY_ID_MAX_LEN: usize = 128;
const ACCESS_KEY_SECRET_MAX_LEN: usize = 256;

/// 只保存 AccessKey；不保存控制台令牌或 API Key。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamCredentials {
    pub access_key_id: String,
    pub access_key_secret: String,
}

pub fn credentials_path(profile_dir: &Path) -> PathBuf {
    profile_dir.join(CREDENTIALS_FILE)
}

/// 校验并写入隔离目录下的 AccessKey。错误信息不回显密钥内容。
pub fn save_credentials(
    profile_dir: &Path,
    access_key_id: &str,
    access_key_secret: &str,
) -> Result<(), String> {
    let access_key_id = access_key_id.trim();
    let access_key_secret = access_key_secret.trim();
    if access_key_id.is_empty() || access_key_secret.is_empty() {
        return Err("AccessKey ID 与 AccessKey Secret 都不能为空。".to_string());
    }
    if access_key_id.len() > ACCESS_KEY_ID_MAX_LEN
        || access_key_secret.len() > ACCESS_KEY_SECRET_MAX_LEN
    {
        return Err("AccessKey 长度超出预期，请检查是否复制了完整内容。".to_string());
    }

    fs::create_dir_all(profile_dir).map_err(|err| format!("创建团队版授权目录失败：{err}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(profile_dir, fs::Permissions::from_mode(0o700))
            .map_err(|err| format!("设置团队版授权目录权限失败：{err}"))?;
    }

    let body = serde_json::to_vec(&TeamCredentials {
        access_key_id: access_key_id.to_string(),
        access_key_secret: access_key_secret.to_string(),
    })
    .map_err(|err| format!("序列化团队版凭证失败：{err}"))?;
    let path = credentials_path(profile_dir);
    let temp = profile_dir.join(format!("{CREDENTIALS_FILE}.tmp"));
    {
        let mut file =
            fs::File::create(&temp).map_err(|err| format!("创建团队版凭证文件失败：{err}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|err| format!("设置团队版凭证权限失败：{err}"))?;
        }
        file.write_all(&body)
            .map_err(|err| format!("写入团队版凭证失败：{err}"))?;
        file.sync_all()
            .map_err(|err| format!("保存团队版凭证失败：{err}"))?;
    }
    #[cfg(windows)]
    {
        // Windows 的 rename 不覆盖已存在文件；重新授权时先移除旧凭证。
        if path.exists() {
            fs::remove_file(&path).map_err(|err| format!("替换团队版凭证失败：{err}"))?;
        }
    }
    fs::rename(&temp, &path).map_err(|err| format!("保存团队版凭证失败：{err}"))
}

pub fn load_credentials(profile_dir: &Path) -> Result<TeamCredentials, BailianError> {
    let raw = fs::read_to_string(credentials_path(profile_dir))
        .map_err(|_| BailianError::auth("未找到团队版 AccessKey，请重新授权。"))?;
    let credentials: TeamCredentials = serde_json::from_str(&raw)
        .map_err(|_| BailianError::auth("团队版凭证文件无法读取，请重新授权。"))?;
    if credentials.access_key_id.trim().is_empty()
        || credentials.access_key_secret.trim().is_empty()
    {
        return Err(BailianError::auth("团队版 AccessKey 已失效，请重新授权。"));
    }
    Ok(credentials)
}

// ---------- 额度模型 ----------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeamSeatUsage {
    /// 席位类型：standard / pro / max。
    pub seat_type: String,
    pub total_seats: Option<i64>,
    pub assigned_seats: Option<i64>,
    /// 该席位类型的总额度与剩余额度，单位 Credits。
    pub total_credits: Option<i64>,
    pub remaining_credits: Option<i64>,
    pub resets_at_epoch: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeamUsage {
    /// 没有有效团队版订阅时保持默认值，`found` 为 false。
    pub found: bool,
    pub start_at_epoch: Option<i64>,
    pub end_at_epoch: Option<i64>,
    pub seats: Vec<TeamSeatUsage>,
}

// ---------- ACS3-HMAC-SHA256 签名 ----------

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    if getrandom::fill(&mut buffer).is_err() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        for (index, byte) in buffer.iter_mut().enumerate() {
            *byte = (nanos.rotate_left(index as u32 % 64) as u8) ^ (index as u8);
        }
    }
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn acs_date(now: OffsetDateTime) -> String {
    now.format(&Rfc3339).unwrap_or_else(|_| {
        // Rfc3339 对 UTC 时间不会失败；兜底成固定格式，避免请求头为空。
        let unix = now.unix_timestamp();
        format!("{unix}")
    })
}

/// 生成签名请求头。`date` 与 `nonce` 由调用方传入，便于单元测试固定输入。
fn signed_headers(
    access_key_id: &str,
    access_key_secret: &str,
    date: &str,
    nonce: &str,
) -> Vec<(&'static str, String)> {
    let content_sha256 = hex_digest(b"");
    let signed_header_names =
        "host;x-acs-action;x-acs-content-sha256;x-acs-date;x-acs-signature-nonce;x-acs-version";
    let canonical_headers = format!(
        "host:{API_HOST}\nx-acs-action:{STATS_ACTION}\nx-acs-content-sha256:{content_sha256}\nx-acs-date:{date}\nx-acs-signature-nonce:{nonce}\nx-acs-version:{ACS_VERSION}\n"
    );
    // CanonicalHeaders 自身以换行结尾；模板里再拼一个换行，得到规范要求的空行。
    let canonical_request = format!(
        "GET\n{STATS_PATH}\n\n{canonical_headers}\n{signed_header_names}\n{content_sha256}"
    );
    let string_to_sign = format!(
        "ACS3-HMAC-SHA256\n{}",
        hex_digest(canonical_request.as_bytes())
    );

    let mut mac = Hmac::<Sha256>::new_from_slice(access_key_secret.as_bytes())
        .expect("HMAC 支持任意长度密钥");
    mac.update(string_to_sign.as_bytes());
    let signature: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();

    let authorization = format!(
        "ACS3-HMAC-SHA256 Credential={access_key_id},SignedHeaders={signed_header_names},Signature={signature}"
    );

    vec![
        ("host", API_HOST.to_string()),
        ("x-acs-action", STATS_ACTION.to_string()),
        ("x-acs-content-sha256", content_sha256),
        ("x-acs-date", date.to_string()),
        ("x-acs-signature-nonce", nonce.to_string()),
        ("x-acs-version", ACS_VERSION.to_string()),
        ("authorization", authorization),
    ]
}

// ---------- 响应解析 ----------

fn as_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number.as_i64().or_else(|| {
            number
                .as_f64()
                .filter(|float| float.is_finite())
                .map(|float| float.round() as i64)
        }),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn millis_to_epoch(value: Option<&Value>) -> Option<i64> {
    as_i64(value).map(|millis| millis.div_euclid(1000))
}

/// Credits 字段是 number，可能带小数；展示按 Credits 取整。
fn credits(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number
            .as_f64()
            .filter(|float| float.is_finite() && *float >= 0.0)
            .map(|float| float.round() as i64),
        Value::String(text) => text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|float| float.is_finite() && *float >= 0.0)
            .map(|float| float.round() as i64),
        _ => None,
    }
}

fn parse_stats(value: &Value) -> Result<TeamUsage, BailianError> {
    let data = value
        .get("Data")
        .or_else(|| value.get("data"))
        .ok_or_else(|| BailianError::decode("团队版响应缺少 Data。"))?;

    let items = data
        .get("Items")
        .or_else(|| data.get("items"))
        .and_then(Value::as_array);

    let mut seats = Vec::new();
    for item in items.into_iter().flatten() {
        let Some(seat_type) = item
            .get("SeatType")
            .or_else(|| item.get("seatType"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        seats.push(TeamSeatUsage {
            seat_type: seat_type.to_string(),
            total_seats: as_i64(item.get("TotalSeats").or_else(|| item.get("totalSeats"))),
            assigned_seats: as_i64(
                item.get("AssignedSeats")
                    .or_else(|| item.get("assignedSeats")),
            ),
            total_credits: credits(item.get("SeatCredits").or_else(|| item.get("seatCredits"))),
            remaining_credits: credits(
                item.get("SeatRemainingCredits")
                    .or_else(|| item.get("seatRemainingCredits")),
            ),
            resets_at_epoch: millis_to_epoch(
                item.get("SeatRefreshTime")
                    .or_else(|| item.get("seatRefreshTime")),
            ),
        });
    }

    let start_at_epoch = millis_to_epoch(
        data.get("SubscriptionStartTime")
            .or_else(|| data.get("subscriptionStartTime")),
    );
    let end_at_epoch = millis_to_epoch(
        data.get("SubscriptionEndTime")
            .or_else(|| data.get("subscriptionEndTime")),
    );

    let found = !seats.is_empty() || start_at_epoch.is_some() || end_at_epoch.is_some();
    Ok(TeamUsage {
        found,
        start_at_epoch,
        end_at_epoch,
        seats,
    })
}

fn api_error(value: &Value) -> BailianError {
    let code = value
        .get("Code")
        .or_else(|| value.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let message = value
        .get("Message")
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if is_auth_code(&code) {
        return BailianError::auth("团队版 AccessKey 无效或已被禁用，请重新授权。");
    }
    if is_permission_code(&code) {
        return BailianError::gateway(
            "当前 AccessKey 没有读取 Token Plan 团队版额度的权限，请为对应 RAM 用户授予 AliyunTokenPlanReadOnlyAccess。",
        );
    }
    BailianError::gateway(format!(
        "团队版接口返回错误：{}",
        if message.is_empty() { code } else { message }
    ))
}

fn is_auth_code(code: &str) -> bool {
    let code = code.to_ascii_lowercase();
    code.contains("invalidaccesskeyid")
        || code.contains("signaturedoesnotmatch")
        || code.contains("invalidsecuritytoken")
        || code.contains("invalidaccesskeysecret")
}

fn is_permission_code(code: &str) -> bool {
    let code = code.to_ascii_lowercase();
    code.contains("nopermission")
        || code.contains("forbidden")
        || code.contains("unauthorizedoperation")
}

// ---------- 查询 ----------

/// 读取团队版订阅与各席位类型的 Credits 统计。
pub fn read_usage(credentials: &TeamCredentials) -> Result<TeamUsage, BailianError> {
    read_usage_at(
        credentials,
        &format!("https://{API_HOST}"),
        OffsetDateTime::now_utc(),
        &random_hex(16),
    )
}

fn read_usage_at(
    credentials: &TeamCredentials,
    base_url: &str,
    now: OffsetDateTime,
    nonce: &str,
) -> Result<TeamUsage, BailianError> {
    let url = format!("{base_url}{STATS_PATH}");
    let headers = signed_headers(
        &credentials.access_key_id,
        &credentials.access_key_secret,
        &acs_date(now),
        nonce,
    );

    let config = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        // 非 2xx 也需要读取响应体里的 Code / Message。
        .http_status_as_error(false)
        .user_agent(concat!("TokenRefill/", env!("CARGO_PKG_VERSION")))
        .build();
    let agent: ureq::Agent = config.into();

    let mut request = agent.get(&url);
    for (name, value) in headers {
        request = request.header(name, &value);
    }
    let mut response = request
        .call()
        .map_err(|err| BailianError::network(format!("连接百炼团队版接口失败：{err}")))?;

    let status = response.status().as_u16();
    let raw = response
        .body_mut()
        .read_to_string()
        .map_err(|err| BailianError::network(format!("读取团队版响应失败：{err}")))?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|_| BailianError::decode("团队版接口响应格式无法识别。"))?;

    if status == 401 {
        return Err(BailianError::auth("团队版 AccessKey 无效，请重新授权。"));
    }
    if status >= 400 {
        return Err(api_error(&value));
    }
    if let Some(false) = value
        .get("Success")
        .or_else(|| value.get("success"))
        .and_then(Value::as_bool)
    {
        return Err(api_error(&value));
    }

    parse_stats(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credentials() -> TeamCredentials {
        TeamCredentials {
            access_key_id: "LTAI-test-id".to_string(),
            access_key_secret: "test-secret".to_string(),
        }
    }

    fn stats_response() -> Value {
        serde_json::json!({
            "Success": true,
            "Code": "200",
            "Message": "successful",
            "Data": {
                "SubscriptionStartTime": 1_787_637_600_000_i64,
                "SubscriptionEndTime": 1_792_908_000_000_i64,
                "Items": [
                    {
                        "SeatType": "standard",
                        "SeatRefreshTime": 1_792_908_000_000_i64,
                        "TotalSeats": 2,
                        "AssignedSeats": 1,
                        "SeatCredits": 25000,
                        "SeatRemainingCredits": 18750
                    },
                    {
                        "SeatType": "pro",
                        "TotalSeats": 1,
                        "AssignedSeats": 1,
                        "SeatCredits": 60000.5,
                        "SeatRemainingCredits": 0
                    }
                ]
            }
        })
    }

    #[test]
    fn stats_are_parsed_per_seat_type() {
        let usage = parse_stats(&stats_response()).unwrap();
        assert!(usage.found);
        assert_eq!(usage.start_at_epoch, Some(1_787_637_600));
        assert_eq!(usage.end_at_epoch, Some(1_792_908_000));
        assert_eq!(usage.seats.len(), 2);

        let standard = &usage.seats[0];
        assert_eq!(standard.seat_type, "standard");
        assert_eq!(standard.total_seats, Some(2));
        assert_eq!(standard.assigned_seats, Some(1));
        assert_eq!(standard.total_credits, Some(25000));
        assert_eq!(standard.remaining_credits, Some(18750));
        assert_eq!(standard.resets_at_epoch, Some(1_792_908_000));

        let pro = &usage.seats[1];
        assert_eq!(pro.total_credits, Some(60001));
        assert_eq!(pro.remaining_credits, Some(0));
        assert_eq!(pro.resets_at_epoch, None);
    }

    #[test]
    fn response_without_items_is_not_an_error() {
        let value = serde_json::json!({
            "Success": true,
            "Data": { "Items": [] }
        });
        let usage = parse_stats(&value).unwrap();
        assert!(!usage.found);
        assert!(usage.seats.is_empty());
    }

    #[test]
    fn missing_fields_stay_unknown_instead_of_zero() {
        let value = serde_json::json!({
            "Success": true,
            "Data": { "Items": [{ "SeatType": "standard" }] }
        });
        let seat = parse_stats(&value).unwrap().seats.remove(0);
        assert_eq!(seat.total_credits, None);
        assert_eq!(seat.remaining_credits, None);
        assert_eq!(seat.total_seats, None);
    }

    #[test]
    fn access_key_errors_are_classified_as_auth_expired() {
        let error = api_error(&serde_json::json!({
            "Code": "InvalidAccessKeyId.NotFound",
            "Message": "Specified access key is not found."
        }));
        assert!(error.is_auth_error());

        let error = api_error(&serde_json::json!({
            "Code": "SignatureDoesNotMatch",
            "Message": "wrong signature"
        }));
        assert!(error.is_auth_error());
    }

    #[test]
    fn permission_errors_point_to_the_ram_policy() {
        let error = api_error(&serde_json::json!({
            "Code": "Forbidden.RAM",
            "Message": "User not authorized to operate on the specified resource."
        }));
        assert!(!error.is_auth_error());
        assert_eq!(error.code, "gateway");
        assert!(error.message.contains("AliyunTokenPlanReadOnlyAccess"));
    }

    /// 固定日期与 nonce 时，签名头必须与独立计算的结果一致。
    /// 期望值由同一套 ACS3 规则在测试外计算后写入，防止实现漂移。
    #[test]
    fn signed_headers_are_deterministic() {
        let date = "2026-01-01T12:00:00Z";
        let headers = signed_headers("LTAI-test-id", "test-secret", date, "0123456789abcdef");
        let map: std::collections::HashMap<_, _> = headers.iter().cloned().collect();
        assert_eq!(map["host"], API_HOST);
        assert_eq!(map["x-acs-action"], STATS_ACTION);
        assert_eq!(map["x-acs-version"], ACS_VERSION);
        assert_eq!(map["x-acs-date"], date);
        assert_eq!(
            map["x-acs-content-sha256"],
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert!(map["authorization"].starts_with(
            "ACS3-HMAC-SHA256 Credential=LTAI-test-id,SignedHeaders=host;x-acs-action;"
        ));
        // 期望签名由同一组规范请求在测试外独立计算得出。
        assert_eq!(
            map["authorization"].split("Signature=").nth(1).unwrap(),
            "97e1e8c72c331d3881467e66532e3e592a953c26f5d9dc000f0da48bce02ec0d"
        );
    }

    #[test]
    fn credentials_round_trip_through_private_directory() {
        let dir = std::env::temp_dir().join(format!(
            "token-refill-team-{}-{}",
            std::process::id(),
            random_hex(4)
        ));
        save_credentials(&dir, "  LTAI-test  ", "  secret  ").unwrap();
        let credentials = load_credentials(&dir).unwrap();
        assert_eq!(credentials.access_key_id, "LTAI-test");
        assert_eq!(credentials.access_key_secret, "secret");

        let raw = fs::read_to_string(credentials_path(&dir)).unwrap();
        assert!(!raw.is_empty());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(credentials_path(&dir))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn empty_access_key_is_rejected_before_writing() {
        let dir = std::env::temp_dir().join(format!(
            "token-refill-team-empty-{}-{}",
            std::process::id(),
            random_hex(4)
        ));
        assert!(save_credentials(&dir, "  ", "secret").is_err());
        assert!(save_credentials(&dir, "LTAI-test", "").is_err());
        assert!(!dir.exists());
    }

    #[test]
    fn missing_credentials_are_reported_as_auth_expired() {
        let dir = std::env::temp_dir().join(format!(
            "token-refill-team-missing-{}-{}",
            std::process::id(),
            random_hex(4)
        ));
        let error = load_credentials(&dir).unwrap_err();
        assert!(error.is_auth_error());
    }

    #[test]
    fn signed_request_is_accepted_by_a_local_mock() {
        use std::io::Read;
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let read = stream.read(&mut chunk).unwrap();
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
                if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let body = r#"{"Success":true,"Data":{"Items":[{"SeatType":"standard","SeatCredits":100,"SeatRemainingCredits":40}]}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
            String::from_utf8_lossy(&buffer).into_owned()
        });

        let usage = read_usage_at(
            &credentials(),
            &format!("http://127.0.0.1:{port}"),
            OffsetDateTime::from_unix_timestamp(1_767_268_800).unwrap(),
            "0123456789abcdef",
        )
        .unwrap();
        assert_eq!(usage.seats[0].remaining_credits, Some(40));

        let request = server.join().unwrap();
        let lower = request.to_ascii_lowercase();
        assert!(request.starts_with("GET /tokenplan/subscription/stats "));
        assert!(lower.contains("authorization: acs3-hmac-sha256 credential=ltai-test-id"));
        assert!(lower.contains("x-acs-action: getsubscriptionstats"));
        assert!(lower.contains("x-acs-version: 2026-02-10"));
        assert!(lower.contains("x-acs-date: 2026-01-01t12:00:00z"));
        assert!(lower.contains("x-acs-signature-nonce: 0123456789abcdef"));
    }

    #[test]
    fn http_errors_are_reported_without_leaking_the_key() {
        use std::io::Read;
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let read = stream.read(&mut chunk).unwrap();
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
                if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let body = r#"{"Code":"InvalidAccessKeyId.NotFound","Message":"Specified access key is not found."}"#;
            let response = format!(
                "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        });

        let error = read_usage_at(
            &credentials(),
            &format!("http://127.0.0.1:{port}"),
            OffsetDateTime::from_unix_timestamp(1_767_268_800).unwrap(),
            "0123456789abcdef",
        )
        .unwrap_err();
        assert!(error.is_auth_error());
        assert!(!error.message.contains("test-secret"));
    }
}
