use crate::package::PACKAGE_MAGIC;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_true() -> bool {
    true
}

fn default_exists_op() -> CheckOperator {
    CheckOperator::Exists
}

fn default_exit_codes() -> Vec<i32> {
    vec![0, 1638, 3010]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegHiveRoot {
    Hklm,
    Hkcu,
    Hkcr,
    Hku,
}

impl Default for RegHiveRoot {
    fn default() -> Self {
        Self::Hklm
    }
}

impl RegHiveRoot {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Hklm => "HKLM",
            Self::Hkcu => "HKCU",
            Self::Hkcr => "HKCR",
            Self::Hku => "HKU",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::Hklm => Self::Hkcu,
            Self::Hkcu => Self::Hkcr,
            Self::Hkcr => Self::Hku,
            Self::Hku => Self::Hklm,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegArchView {
    Default,
    X64,
    X86,
}

impl Default for RegArchView {
    fn default() -> Self {
        Self::X64
    }
}

impl RegArchView {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Default => "默认视图",
            Self::X64 => "64位视图",
            Self::X86 => "32位视图",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::Default => Self::X64,
            Self::X64 => Self::X86,
            Self::X86 => Self::Default,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckOperator {
    Exists,
    NotExists,
    Equals,
    NotEquals,
    GreaterOrEqual,
    Contains,
}

impl Default for CheckOperator {
    fn default() -> Self {
        Self::Equals
    }
}

impl CheckOperator {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Exists => "存在",
            Self::NotExists => "不存在",
            Self::Equals => "等于 (==)",
            Self::NotEquals => "不等于 (!=)",
            Self::GreaterOrEqual => "版本/数值 >=",
            Self::Contains => "包含文本",
        }
    }

    pub fn next_for_registry(&self) -> Self {
        match self {
            Self::Equals => Self::GreaterOrEqual,
            Self::GreaterOrEqual => Self::Exists,
            Self::Exists => Self::NotExists,
            Self::NotExists => Self::Contains,
            Self::Contains => Self::NotEquals,
            Self::NotEquals => Self::Equals,
        }
    }

    pub fn next_for_file(&self) -> Self {
        match self {
            Self::Exists => Self::GreaterOrEqual,
            Self::GreaterOrEqual => Self::Equals,
            Self::Equals => Self::NotExists,
            _ => Self::Exists,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PrereqCondition {
    Registry {
        #[serde(default)]
        root: RegHiveRoot,
        subkey: String,
        #[serde(default)]
        value_name: String,
        #[serde(default)]
        arch_view: RegArchView,
        #[serde(default)]
        operator: CheckOperator,
        #[serde(default)]
        expected: String,
    },
    File {
        path: String,
        #[serde(default = "default_exists_op")]
        operator: CheckOperator,
        #[serde(default)]
        expected_version: String,
    },
}

impl PrereqCondition {
    pub fn is_registry(&self) -> bool {
        matches!(self, Self::Registry { .. })
    }

    pub fn short_tag(&self) -> &'static str {
        match self {
            Self::Registry { .. } => "注册表",
            Self::File { .. } => "文件/PE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PrereqAction {
    RunBundledExe {
        source_file: String,
        #[serde(default)]
        bundled_rel_path: String,
        #[serde(default)]
        args: String,
        #[serde(default = "default_true")]
        elevate: bool,
        #[serde(default = "default_true")]
        wait: bool,
        #[serde(default = "default_exit_codes")]
        success_exit_codes: Vec<i32>,
    },
    RunCommand {
        program: String,
        #[serde(default)]
        args: String,
        #[serde(default)]
        elevate: bool,
        #[serde(default = "default_true")]
        wait: bool,
        #[serde(default = "default_exit_codes")]
        success_exit_codes: Vec<i32>,
    },
    PromptAndAbort {
        message: String,
        #[serde(default)]
        download_url: String,
    },
}

impl PrereqAction {
    pub fn short_tag(&self) -> &'static str {
        match self {
            Self::RunBundledExe { .. } => "捆绑安装",
            Self::RunCommand { .. } => "执行命令",
            Self::PromptAndAbort { .. } => "弹窗拦截",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrerequisiteRule {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub condition: PrereqCondition,
    pub action: PrereqAction,
}

impl PrerequisiteRule {
    pub fn preset_vc_redist_x64() -> Self {
        Self {
            name: "VC++ 2015-2022 (x64)".to_string(),
            enabled: true,
            condition: PrereqCondition::Registry {
                root: RegHiveRoot::Hklm,
                subkey: "SOFTWARE\\Microsoft\\VisualStudio\\14.0\\VC\\Runtimes\\x64".to_string(),
                value_name: "Installed".to_string(),
                arch_view: RegArchView::X64,
                operator: CheckOperator::Equals,
                expected: "1".to_string(),
            },
            action: PrereqAction::RunBundledExe {
                source_file: "vc_redist.x64.exe".to_string(),
                bundled_rel_path: String::new(),
                args: "/install /quiet /norestart".to_string(),
                elevate: true,
                wait: true,
                success_exit_codes: default_exit_codes(),
            },
        }
    }

    pub fn preset_vc_redist_x86() -> Self {
        Self {
            name: "VC++ 2015-2022 (x86)".to_string(),
            enabled: true,
            condition: PrereqCondition::Registry {
                root: RegHiveRoot::Hklm,
                subkey: "SOFTWARE\\Microsoft\\VisualStudio\\14.0\\VC\\Runtimes\\x86".to_string(),
                value_name: "Installed".to_string(),
                arch_view: RegArchView::X86,
                operator: CheckOperator::Equals,
                expected: "1".to_string(),
            },
            action: PrereqAction::RunBundledExe {
                source_file: "vc_redist.x86.exe".to_string(),
                bundled_rel_path: String::new(),
                args: "/install /quiet /norestart".to_string(),
                elevate: true,
                wait: true,
                success_exit_codes: default_exit_codes(),
            },
        }
    }

    pub fn preset_custom_registry() -> Self {
        Self {
            name: "WebView2 注册表检测".to_string(),
            enabled: true,
            condition: PrereqCondition::Registry {
                root: RegHiveRoot::Hklm,
                subkey: "SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}".to_string(),
                value_name: "pv".to_string(),
                arch_view: RegArchView::X86,
                operator: CheckOperator::GreaterOrEqual,
                expected: "100.0.0.0".to_string(),
            },
            action: PrereqAction::PromptAndAbort {
                message: "系统缺少 Microsoft Edge WebView2 运行时，请先安装后再运行本安装程序。".to_string(),
                download_url: "https://developer.microsoft.com/microsoft-edge/webview2/".to_string(),
            },
        }
    }

    pub fn preset_custom_file_version() -> Self {
        Self {
            name: "vcruntime140 版本检测".to_string(),
            enabled: true,
            condition: PrereqCondition::File {
                path: "%SystemRoot%\\System32\\vcruntime140_1.dll".to_string(),
                operator: CheckOperator::GreaterOrEqual,
                expected_version: "14.30.0.0".to_string(),
            },
            action: PrereqAction::RunBundledExe {
                source_file: "vc_redist.x64.exe".to_string(),
                bundled_rel_path: String::new(),
                args: "/install /quiet /norestart".to_string(),
                elevate: true,
                wait: true,
                success_exit_codes: default_exit_codes(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessConflictPolicy {
    /// 自动静默结束正在运行的目标程序进程并继续安装
    AutoKill,
    /// 弹出提示窗口列出占用的进程，提供「重试」与「结束进程并重试」两种选择
    PromptUser,
}

impl Default for ProcessConflictPolicy {
    fn default() -> Self {
        Self::AutoKill
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PackageConfig {
    pub app_id: String,
    pub app_name: String,
    pub app_version: String,
    pub publisher: String,
    pub main_exe: String,
    pub default_install_dir: String,
    pub license_text: String,
    pub create_desktop_shortcut: bool,
    pub create_start_menu_shortcut: bool,
    pub register_uninstaller: bool,
    pub auto_launch_after_install: bool,
    #[serde(default)]
    pub process_conflict_policy: ProcessConflictPolicy,
    #[serde(default = "default_true")]
    pub kill_process_on_update: bool,
    pub compression_level: u32,
    #[serde(default)]
    pub installer_icon: Option<String>,
    #[serde(default)]
    pub prerequisites: Vec<PrerequisiteRule>,
}

impl Default for PackageConfig {
    fn default() -> Self {
        Self {
            app_id: "com.nyansoft.demoapp".to_string(),
            app_name: "Nyan Studio App".to_string(),
            app_version: "1.0.0".to_string(),
            publisher: "Nyan Softworks Inc.".to_string(),
            main_exe: "NyanApp.exe".to_string(),
            default_install_dir: "%LOCALAPPDATA%\\Programs\\Nyan Studio App".to_string(),
            license_text: DEFAULT_LICENSE_TEXT.to_string(),
            create_desktop_shortcut: true,
            create_start_menu_shortcut: true,
            register_uninstaller: true,
            auto_launch_after_install: true,
            process_conflict_policy: ProcessConflictPolicy::AutoKill,
            kill_process_on_update: true,
            compression_level: 6,
            installer_icon: None,
            prerequisites: Vec::new(),
        }
    }
}

pub const DEFAULT_LICENSE_TEXT: &str = r#"终端用户软件许可协议 (EULA)

重要提示——请仔细阅读：
本《终端用户软件许可协议》（以下简称“本协议”）是您（个人或单一实体）与软件发布者之间就安装、复制或以其他方式使用本软件产品所订立的法律协议。

1. 许可授予
在您遵守本协议条款的前提下，发布者授予您一项个人的、非排他性的、不可转让的有限许可，允许您在您拥有或控制的计算机设备上安装并运行本软件。

2. 版权与知识产权
本软件受著作权法及国际著作权条约和其他知识产权法律及条约的保护。本软件只授予使用许可，而非出售。

3. 使用限制
您不得对本软件进行反向工程、反编译或反汇编；不得出租、租赁或出借本软件；不得移除本软件上的任何版权或所有权声明。

4. 免责声明
本软件按“现状”提供，不附带任何明示或暗示的担保，包括但不限于对适销性、特定用途适用性及不侵权的暗示担保。

勾选“我已阅读并同意软件许可协议”并继续安装，即表示您同意接受本协议全部条款的约束。"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntryMetadata {
    pub relative_path: String,
    pub uncompressed_size: u64,
    pub solid_offset: u64,
    pub crc32: u32,
    pub bcj_filtered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub config: PackageConfig,
    pub files: Vec<FileEntryMetadata>,
    pub total_uncompressed_bytes: u64,
    pub total_compressed_bytes: u64,
    pub dict_size: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct NyanFooter {
    pub stub_size: u64,
    pub data_offset: u64,
    pub manifest_offset: u64,
    pub manifest_size: u64,
}

impl NyanFooter {
    pub fn to_bytes(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..8].copy_from_slice(&self.stub_size.to_le_bytes());
        buf[8..16].copy_from_slice(&self.data_offset.to_le_bytes());
        buf[16..24].copy_from_slice(&self.manifest_offset.to_le_bytes());
        buf[24..32].copy_from_slice(&self.manifest_size.to_le_bytes());
        buf[32..40].copy_from_slice(PACKAGE_MAGIC);
        buf
    }

    pub fn from_bytes(buf: &[u8; 40]) -> Option<Self> {
        if &buf[32..40] != PACKAGE_MAGIC {
            return None;
        }
        Some(Self {
            stub_size: u64::from_le_bytes(buf[0..8].try_into().ok()?),
            data_offset: u64::from_le_bytes(buf[8..16].try_into().ok()?),
            manifest_offset: u64::from_le_bytes(buf[16..24].try_into().ok()?),
            manifest_size: u64::from_le_bytes(buf[24..32].try_into().ok()?),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UninstallReceipt {
    pub app_id: String,
    pub app_name: String,
    pub app_version: String,
    pub publisher: String,
    pub install_dir: String,
    pub installed_files: Vec<String>,
    pub desktop_shortcut: Option<String>,
    pub start_menu_shortcut: Option<String>,
    pub registry_key_created: bool,
}

#[derive(Debug, Clone)]
pub enum BuildProgressEvent {
    Scanning(usize),
    Compressing {
        current_index: usize,
        total_files: usize,
        relative_path: String,
    },
    Finalizing,
    Completed {
        output_path: PathBuf,
        total_files: usize,
        uncompressed_bytes: u64,
        final_installer_bytes: u64,
    },
    Error(String),
}

#[derive(Debug, Clone)]
pub enum InstallProgressEvent {
    KillingProcesses(String),
    Extracting {
        current_index: usize,
        total_files: usize,
        relative_path: String,
        bytes_done: u64,
        bytes_total: u64,
    },
    RunningPrerequisite(String),
    CreatingShortcuts,
    RegisteringUninstaller,
    Completed {
        install_dir: PathBuf,
    },
    Error(String),
}
