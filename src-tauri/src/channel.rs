//! 分发渠道。
//!
//! 正式版与测试版共用同一套代码，靠 `tauri.test.conf.json` 覆盖
//! `productName` / `identifier` 来隔离安装、数据目录与系统权限。
//! 运行时只以合并后的 `identifier` 判定渠道，避免再多一份构建期开关。

use tauri::AppHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Pro,
    Test,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pro => "pro",
            Self::Test => "test",
        }
    }

    pub fn is_test(self) -> bool {
        matches!(self, Self::Test)
    }
}

/// 测试渠道的标识统一以 `.test` 结尾；正式版保持 `dev.tokenrefill.desktop`。
pub fn from_identifier(identifier: &str) -> Channel {
    if identifier.ends_with(".test") {
        Channel::Test
    } else {
        Channel::Pro
    }
}

pub fn of(app: &AppHandle) -> Channel {
    from_identifier(&app.config().identifier)
}

pub fn is_test(app: &AppHandle) -> bool {
    of(app).is_test()
}

#[cfg(test)]
mod tests {
    use super::{from_identifier, Channel};

    #[test]
    fn test_channel_is_derived_from_identifier_suffix() {
        assert_eq!(from_identifier("dev.tokenrefill.desktop"), Channel::Pro);
        assert_eq!(
            from_identifier("dev.tokenrefill.desktop.test"),
            Channel::Test
        );
        // 只认完整的 `.test` 后缀，避免误伤普通标识。
        assert_eq!(from_identifier("dev.tokenrefill.testing"), Channel::Pro);
        assert_eq!(
            from_identifier("dev.tokenrefill.desktop.testing"),
            Channel::Pro
        );
    }

    #[test]
    fn channel_strings_are_stable() {
        assert_eq!(Channel::Pro.as_str(), "pro");
        assert_eq!(Channel::Test.as_str(), "test");
        assert!(Channel::Test.is_test());
        assert!(!Channel::Pro.is_test());
    }
}
