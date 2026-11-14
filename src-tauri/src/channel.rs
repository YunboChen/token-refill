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
    /// 大版本改造的预览渠道；与测试版一样不参与应用内更新。
    Next,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pro => "pro",
            Self::Test => "test",
            Self::Next => "next",
        }
    }

    /// 测试版与 Next 预览版都不检查、不安装应用内更新。
    pub fn is_preview(self) -> bool {
        matches!(self, Self::Test | Self::Next)
    }
}

/// 测试渠道以 `.test` 结尾，Next 预览渠道以 `.next` 结尾；
/// 正式版保持 `dev.tokenrefill.desktop`。
pub fn from_identifier(identifier: &str) -> Channel {
    if identifier.ends_with(".test") {
        Channel::Test
    } else if identifier.ends_with(".next") {
        Channel::Next
    } else {
        Channel::Pro
    }
}

pub fn of(app: &AppHandle) -> Channel {
    from_identifier(&app.config().identifier)
}

pub fn is_preview(app: &AppHandle) -> bool {
    of(app).is_preview()
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
        assert_eq!(
            from_identifier("dev.tokenrefill.desktop.next"),
            Channel::Next
        );
        // 只认完整的 `.test` / `.next` 后缀，避免误伤普通标识。
        assert_eq!(from_identifier("dev.tokenrefill.testing"), Channel::Pro);
        assert_eq!(
            from_identifier("dev.tokenrefill.desktop.testing"),
            Channel::Pro
        );
        assert_eq!(
            from_identifier("dev.tokenrefill.desktop.nextgen"),
            Channel::Pro
        );
    }

    #[test]
    fn channel_strings_are_stable() {
        assert_eq!(Channel::Pro.as_str(), "pro");
        assert_eq!(Channel::Test.as_str(), "test");
        assert_eq!(Channel::Next.as_str(), "next");
        assert!(Channel::Test.is_preview());
        assert!(Channel::Next.is_preview());
        assert!(!Channel::Pro.is_preview());
    }
}
