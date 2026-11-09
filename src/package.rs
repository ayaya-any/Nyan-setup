use crc32fast::Hasher;
use lzma_rust::{CountingWriter, LZMA2Options, LZMA2Reader, LZMA2Writer};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

pub const PACKAGE_MAGIC: &[u8; 8] = b"NYANPK02";
pub const FOOTER_SIZE: u64 = 40;
pub const UNINSTALL_DATA_FILE: &str = "unins000.dat";
pub const UNINSTALL_EXE_FILE: &str = "unins000.exe";
pub const PREREQ_VIRTUAL_PREFIX: &str = "__nyan_prereq__/";

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
        let stub_size = u64::from_le_bytes(buf[0..8].try_into().ok()?);
        let data_offset = u64::from_le_bytes(buf[8..16].try_into().ok()?);
        let manifest_offset = u64::from_le_bytes(buf[16..24].try_into().ok()?);
        let manifest_size = u64::from_le_bytes(buf[24..32].try_into().ok()?);
        Some(Self {
            stub_size,
            data_offset,
            manifest_offset,
            manifest_size,
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

/// Detect whether a file is a Windows PE executable/library (`MZ` header) eligible for x86 BCJ pre-filtering.
pub fn should_apply_x86_bcj(rel_path: &str, data: &[u8]) -> bool {
    if data.len() < 64 || &data[0..2] != b"MZ" {
        return false;
    }
    let lower = rel_path.to_ascii_lowercase();
    lower.ends_with(".exe")
        || lower.ends_with(".dll")
        || lower.ends_with(".sys")
        || lower.ends_with(".ocx")
}

/// Reversible x86 BCJ (LZX/CAB style E8/E9 Call/Jump) pre-filter:
/// Converts relative 32-bit jump targets pointing inside `[0, file_size)` to absolute addresses.
/// Strictly bijective over non-overlapping 5-byte instruction windows.
pub fn bcj_x86_encode(buf: &mut [u8]) {
    if buf.len() < 5 || buf.len() > (i32::MAX as usize / 2) {
        return;
    }
    let file_size = buf.len() as i32;
    let limit = buf.len() - 4;
    let mut i = 0usize;
    while i < limit {
        let b = buf[i];
        if b == 0xE8 || b == 0xE9 {
            let pos = (i + 5) as i32;
            let val = i32::from_le_bytes([buf[i + 1], buf[i + 2], buf[i + 3], buf[i + 4]]);
            if val >= -pos && val < (file_size - pos) {
                let abs = val + pos;
                buf[i + 1..i + 5].copy_from_slice(&abs.to_le_bytes());
            } else if val >= (file_size - pos) && val < file_size {
                let shifted = val - file_size;
                buf[i + 1..i + 5].copy_from_slice(&shifted.to_le_bytes());
            }
            i += 5;
        } else {
            i += 1;
        }
    }
}

/// Inverse x86 BCJ filter: restores absolute addresses back to relative `0xE8`/`0xE9` offsets.
pub fn bcj_x86_decode(buf: &mut [u8]) {
    if buf.len() < 5 || buf.len() > (i32::MAX as usize / 2) {
        return;
    }
    let file_size = buf.len() as i32;
    let limit = buf.len() - 4;
    let mut i = 0usize;
    while i < limit {
        let b = buf[i];
        if b == 0xE8 || b == 0xE9 {
            let pos = (i + 5) as i32;
            let val = i32::from_le_bytes([buf[i + 1], buf[i + 2], buf[i + 3], buf[i + 4]]);
            if val >= 0 && val < file_size {
                let rel = val - pos;
                buf[i + 1..i + 5].copy_from_slice(&rel.to_le_bytes());
            } else if val >= -pos && val < 0 {
                let unshifted = val + file_size;
                buf[i + 1..i + 5].copy_from_slice(&unshifted.to_le_bytes());
            }
            i += 5;
        } else {
            i += 1;
        }
    }
}

fn lzma2_compress_bytes(data: &[u8], preset: u32) -> io::Result<(Vec<u8>, u32)> {
    let opts = LZMA2Options::with_preset(preset.clamp(0, 9));
    let dict_size = opts.dict_size;
    let mut out = Vec::new();
    {
        let mut writer = LZMA2Writer::new(CountingWriter::new(&mut out), &opts);
        writer.write_all(data)?;
        writer.finish()?;
    }
    Ok((out, dict_size))
}

fn lzma2_decompress_bytes(compressed: &[u8], dict_size: u32) -> io::Result<Vec<u8>> {
    let mut reader = LZMA2Reader::new(compressed, dict_size, None);
    let mut out = Vec::new();
    reader.read_to_end(&mut out)?;
    Ok(out)
}

/// Inspect an executable file to check if it contains an attached NyanSetup payload (`NYANPK02`).
pub fn read_package_from_exe(exe_path: &Path) -> io::Result<Option<(NyanFooter, PackageManifest)>> {
    let mut file = File::open(exe_path)?;
    let file_len = file.metadata()?.len();
    if file_len < FOOTER_SIZE + 1024 {
        return Ok(None);
    }

    file.seek(SeekFrom::End(-(FOOTER_SIZE as i64)))?;
    let mut footer_buf = [0u8; 40];
    file.read_exact(&mut footer_buf)?;

    let footer = match NyanFooter::from_bytes(&footer_buf) {
        Some(f) => f,
        None => return Ok(None),
    };

    if footer.manifest_offset + footer.manifest_size + FOOTER_SIZE > file_len
        || footer.stub_size == 0
        || footer.stub_size > footer.data_offset
    {
        return Ok(None);
    }

    file.seek(SeekFrom::Start(footer.manifest_offset))?;
    let mut compressed_manifest = vec![0u8; footer.manifest_size as usize];
    file.read_exact(&mut compressed_manifest)?;

    let manifest_bytes =
        lzma2_decompress_bytes(&compressed_manifest, LZMA2Options::DICT_SIZE_DEFAULT)?;
    let manifest: PackageManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    Ok(Some((footer, manifest)))
}

/// Extract the pure PE stub bytes from the currently running executable.
pub fn extract_pure_stub_bytes(exe_path: &Path) -> io::Result<Vec<u8>> {
    let mut file = File::open(exe_path)?;
    let file_len = file.metadata()?.len();

    let stub_len = if let Ok(Some((footer, _))) = read_package_from_exe(exe_path) {
        footer.stub_size
    } else {
        file_len
    };

    file.seek(SeekFrom::Start(0))?;
    let mut buf = vec![0u8; stub_len as usize];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

/// Recursively collect all files inside `source_dir`, grouped by file extension for optimal Solid Compression ratio.
pub fn collect_source_files(source_dir: &Path) -> io::Result<Vec<(PathBuf, String, u64)>> {
    let mut results = Vec::new();
    if !source_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("源目录不存在: {}", source_dir.display()),
        ));
    }

    fn visit_dirs(
        root: &Path,
        current: &Path,
        out: &mut Vec<(PathBuf, String, u64)>,
    ) -> io::Result<()> {
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                visit_dirs(root, &path, out)?;
            } else if metadata.is_file() {
                let rel = path
                    .strip_prefix(root)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
                    .to_string_lossy()
                    .replace('/', "\\");
                out.push((path, rel, metadata.len()));
            }
        }
        Ok(())
    }

    visit_dirs(source_dir, source_dir, &mut results)?;

    // Sort by extension first (so .exe/.dll group together in the solid LZMA2 dictionary), then by relative path
    results.sort_by(|a, b| {
        let ext_a = Path::new(&a.1)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let ext_b = Path::new(&b.1)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        ext_a.cmp(&ext_b).then_with(|| a.1.cmp(&b.1))
    });

    Ok(results)
}

/// Build a single-file self-extracting installer `.exe` using **Solid LZMA2 + x86 BCJ Pre-Filter**.
pub fn build_installer_package(
    stub_exe_path: &Path,
    source_dir: &Path,
    output_exe_path: &Path,
    config: &PackageConfig,
    progress_tx: Option<&Sender<BuildProgressEvent>>,
) -> Result<(usize, u64, u64), String> {
    let notify = |ev: BuildProgressEvent| {
        if let Some(tx) = progress_tx {
            let _ = tx.send(ev);
        }
    };

    let mut files = collect_source_files(source_dir).map_err(|e| format!("扫描源目录失败: {}", e))?;
    if files.is_empty() {
        return Err("源目录中没有可打包的文件，请检查源目录路径。".to_string());
    }

    // Resolve and append any bundled prerequisite executables
    let mut manifest_config = config.clone();
    for (rule_idx, rule) in manifest_config.prerequisites.iter_mut().enumerate() {
        if !rule.enabled {
            continue;
        }
        if let PrereqAction::RunBundledExe {
            ref source_file,
            ref mut bundled_rel_path,
            ..
        } = rule.action
        {
            let trimmed = source_file.trim();
            if trimmed.is_empty() {
                return Err(format!("前置规则 [{}] 未指定捆绑可执行文件路径", rule.name));
            }
            let cand = PathBuf::from(trimmed);
            let resolved = if cand.is_file() {
                cand
            } else if source_dir.join(&cand).is_file() {
                source_dir.join(&cand)
            } else {
                return Err(format!(
                    "前置规则 [{}] 指定的捆绑程序不存在: {}",
                    rule.name, trimmed
                ));
            };
            let meta = fs::metadata(&resolved).map_err(|e| {
                format!(
                    "读取前置规则 [{}] 捆绑程序 {} 失败: {}",
                    rule.name,
                    resolved.display(),
                    e
                )
            })?;
            let fname = resolved
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("prereq_{}.exe", rule_idx));
            let v_rel = format!("{}prereq_{}_{}", PREREQ_VIRTUAL_PREFIX, rule_idx, fname);
            *bundled_rel_path = v_rel.clone();
            files.push((resolved, v_rel, meta.len()));
        }
    }

    notify(BuildProgressEvent::Scanning(files.len()));

    if let Some(parent) = output_exe_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("无法创建输出目录: {}", e))?;
        }
    }

    let output_canonical = output_exe_path.canonicalize().ok();

    let mut stub_bytes = extract_pure_stub_bytes(stub_exe_path)
        .map_err(|e| format!("读取安装器 Stub 模板失败: {}", e))?;

    // If custom installer icon is specified, apply it to the PE stub resources
    if let Some(ref icon_str) = config.installer_icon {
        let trimmed = icon_str.trim();
        if !trimmed.is_empty() {
            let icon_path = PathBuf::from(trimmed);
            let resolved_path = if icon_path.is_file() {
                icon_path
            } else if source_dir.join(&icon_path).is_file() {
                source_dir.join(&icon_path)
            } else {
                return Err(format!("指定的安装包图标不存在: {}", trimmed));
            };

            let ico_data = fs::read(&resolved_path)
                .map_err(|e| format!("读取自定义图标文件 {} 失败: {}", resolved_path.display(), e))?;
            stub_bytes = crate::icon::apply_icon_to_stub_bytes(&stub_bytes, &ico_data)
                .map_err(|e| format!("应用自定义安装包图标失败: {}", e))?;
        }
    }

    let stub_size = stub_bytes.len() as u64;

    let mut out_file = File::create(output_exe_path).map_err(|e| {
        format!(
            "无法创建输出文件 {}: {}",
            output_exe_path.display(),
            e
        )
    })?;
    out_file
        .write_all(&stub_bytes)
        .map_err(|e| format!("写入 Stub 头部失败: {}", e))?;

    let data_offset = stub_size;
    let preset = config.compression_level.clamp(1, 9);
    let opts = LZMA2Options::with_preset(preset);
    let dict_size = opts.dict_size;

    let mut solid_compressed_data = Vec::new();
    let mut file_entries = Vec::with_capacity(files.len());
    let mut solid_offset = 0u64;
    let mut total_uncompressed_bytes = 0u64;

    {
        let mut solid_encoder =
            LZMA2Writer::new(CountingWriter::new(&mut solid_compressed_data), &opts);

        let total_files = files.len();
        for (idx, (full_path, rel_path, _size)) in files.into_iter().enumerate() {
            if let (Some(ref out_can), Ok(ref cur_can)) =
                (&output_canonical, full_path.canonicalize())
            {
                if out_can == cur_can {
                    continue;
                }
            }

            notify(BuildProgressEvent::Compressing {
                current_index: idx + 1,
                total_files,
                relative_path: rel_path.clone(),
            });

            let mut raw_data =
                fs::read(&full_path).map_err(|e| format!("读取文件 {} 失败: {}", rel_path, e))?;
            let uncompressed_size = raw_data.len() as u64;

            // Compute CRC32 on original untouched bytes
            let mut hasher = Hasher::new();
            hasher.update(&raw_data);
            let crc32 = hasher.finalize();

            // Apply x86 BCJ pre-filter on PE binaries (.exe / .dll) before feeding into solid LZMA2 stream
            let bcj_filtered = should_apply_x86_bcj(&rel_path, &raw_data);
            if bcj_filtered {
                bcj_x86_encode(&mut raw_data);
            }

            solid_encoder
                .write_all(&raw_data)
                .map_err(|e| format!("LZMA2 固实压缩写入 {} 失败: {}", rel_path, e))?;

            file_entries.push(FileEntryMetadata {
                relative_path: rel_path,
                uncompressed_size,
                solid_offset,
                crc32,
                bcj_filtered,
            });

            solid_offset += uncompressed_size;
            total_uncompressed_bytes += uncompressed_size;
        }

        solid_encoder
            .finish()
            .map_err(|e| format!("完成 LZMA2 固实压缩流失败: {}", e))?;
    }

    let total_compressed_bytes = solid_compressed_data.len() as u64;
    out_file
        .write_all(&solid_compressed_data)
        .map_err(|e| format!("写入固实压缩数据块失败: {}", e))?;

    notify(BuildProgressEvent::Finalizing);

    let manifest = PackageManifest {
        config: manifest_config,
        files: file_entries,
        total_uncompressed_bytes,
        total_compressed_bytes,
        dict_size,
    };

    let manifest_json =
        serde_json::to_vec(&manifest).map_err(|e| format!("序列化安装包清单失败: {}", e))?;
    let (compressed_manifest, _) = lzma2_compress_bytes(&manifest_json, 6)
        .map_err(|e| format!("LZMA2 压缩清单失败: {}", e))?;

    let manifest_offset = data_offset + total_compressed_bytes;
    let manifest_size = compressed_manifest.len() as u64;

    out_file
        .write_all(&compressed_manifest)
        .map_err(|e| format!("写入清单数据失败: {}", e))?;

    let footer = NyanFooter {
        stub_size,
        data_offset,
        manifest_offset,
        manifest_size,
    };
    out_file
        .write_all(&footer.to_bytes())
        .map_err(|e| format!("写入安装包尾标失败: {}", e))?;
    out_file
        .flush()
        .map_err(|e| format!("刷新文件缓冲失败: {}", e))?;

    let final_size = stub_size + total_compressed_bytes + manifest_size + FOOTER_SIZE;
    notify(BuildProgressEvent::Completed {
        output_path: output_exe_path.to_path_buf(),
        total_files: manifest.files.len(),
        uncompressed_bytes: total_uncompressed_bytes,
        final_installer_bytes: final_size,
    });

    Ok((manifest.files.len(), total_uncompressed_bytes, final_size))
}

/// Extract all packaged files from the Solid LZMA2 stream inside `installer_exe_path` into `target_install_dir`.
pub fn extract_package_files(
    installer_exe_path: &Path,
    manifest: &PackageManifest,
    target_install_dir: &Path,
    kill_locked_processes: bool,
    progress_tx: Option<&Sender<InstallProgressEvent>>,
) -> Result<Vec<String>, String> {
    let mut archive_file =
        File::open(installer_exe_path).map_err(|e| format!("无法打开安装包文件: {}", e))?;

    let (footer, _) = read_package_from_exe(installer_exe_path)
        .map_err(|e| format!("读取安装包尾标失败: {}", e))?
        .ok_or_else(|| "安装包尾标损坏或格式不受支持".to_string())?;

    // 1. Pre-check Prerequisite rules:
    //    - PromptAndAbort rules fail fast before touching disk
    //    - RunBundledExe rules determine which bundled installers need extraction to %TEMP%
    let mut needed_bundled_prereqs: Vec<(usize, String, PathBuf)> = Vec::new();
    for (rule_idx, rule) in manifest.config.prerequisites.iter().enumerate() {
        if !rule.enabled {
            continue;
        }
        let passed =
            crate::win32_ops::evaluate_prereq_condition(&rule.condition, Some(target_install_dir));
        if !passed {
            match rule.action {
                PrereqAction::PromptAndAbort {
                    ref message,
                    ref download_url,
                } => {
                    crate::win32_ops::prompt_prereq_abort(&rule.name, message, download_url);
                    return Err(format!("环境检测未通过 [{}]: {}", rule.name, message));
                }
                PrereqAction::RunBundledExe {
                    ref bundled_rel_path,
                    ref source_file,
                    ..
                } => {
                    let rel_key = if !bundled_rel_path.is_empty() {
                        bundled_rel_path.clone()
                    } else {
                        let fname = Path::new(source_file)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| format!("prereq_{}.exe", rule_idx));
                        format!("{}prereq_{}_{}", PREREQ_VIRTUAL_PREFIX, rule_idx, fname)
                    };
                    let safe_fname = Path::new(&rel_key)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| format!("prereq_{}.exe", rule_idx));
                    let temp_dest = std::env::temp_dir().join(format!(
                        "nyan_setup_{}_{}_{}",
                        std::process::id(),
                        rule_idx,
                        safe_fname
                    ));
                    needed_bundled_prereqs.push((rule_idx, rel_key, temp_dest));
                }
                PrereqAction::RunCommand { .. } => {}
            }
        }
    }

    let mut candidate_files: Vec<String> = manifest
        .files
        .iter()
        .filter(|f| !f.relative_path.starts_with(PREREQ_VIRTUAL_PREFIX))
        .map(|f| f.relative_path.clone())
        .collect();
    if !manifest.config.main_exe.is_empty()
        && !candidate_files.contains(&manifest.config.main_exe)
    {
        candidate_files.push(manifest.config.main_exe.clone());
    }

    if kill_locked_processes {
        let killed_msgs = crate::win32_ops::terminate_running_target_processes(
            target_install_dir,
            &candidate_files,
        );
        if let Some(tx) = progress_tx {
            for msg in killed_msgs {
                let _ = tx.send(InstallProgressEvent::KillingProcesses(msg));
            }
        }
    }

    fs::create_dir_all(target_install_dir)
        .map_err(|e| format!("无法创建目标安装目录 {}: {}", target_install_dir.display(), e))?;

    archive_file
        .seek(SeekFrom::Start(footer.data_offset))
        .map_err(|e| format!("定位固实压缩流起点失败: {}", e))?;

    let solid_stream = archive_file.take(manifest.total_compressed_bytes);
    let mut lzma_reader = LZMA2Reader::new(solid_stream, manifest.dict_size.max(4096), None);

    let total_files = manifest.files.len();
    let bytes_total = manifest.total_uncompressed_bytes.max(1);
    let mut bytes_done = 0u64;
    let mut installed_files = Vec::with_capacity(total_files);

    for (idx, entry) in manifest.files.iter().enumerate() {
        if let Some(tx) = progress_tx {
            let _ = tx.send(InstallProgressEvent::Extracting {
                current_index: idx + 1,
                total_files,
                relative_path: entry.relative_path.clone(),
                bytes_done,
                bytes_total,
            });
        }

        // Handle bundled prerequisite files
        if entry.relative_path.starts_with(PREREQ_VIRTUAL_PREFIX) {
            if let Some((_, _, ref temp_dest)) = needed_bundled_prereqs
                .iter()
                .find(|(_, rel_key, _)| rel_key == &entry.relative_path)
            {
                let mut raw_buf = vec![0u8; entry.uncompressed_size as usize];
                lzma_reader.read_exact(&mut raw_buf).map_err(|e| {
                    format!("解压前置组件 {} 失败: {}", entry.relative_path, e)
                })?;
                if entry.bcj_filtered {
                    bcj_x86_decode(&mut raw_buf);
                }
                let mut hasher = Hasher::new();
                hasher.update(&raw_buf);
                let actual_crc = hasher.finalize();
                if actual_crc != entry.crc32 {
                    return Err(format!(
                        "前置组件 {} CRC32 校验失败 (期望 {:08X}, 实际 {:08X})",
                        entry.relative_path, entry.crc32, actual_crc
                    ));
                }
                fs::write(temp_dest, &raw_buf).map_err(|e| {
                    format!("释放临时前置安装包 {} 失败: {}", temp_dest.display(), e)
                })?;
            } else {
                // Prerequisite already satisfied on system -> discard stream bytes without disk write
                io::copy(
                    &mut (&mut lzma_reader).take(entry.uncompressed_size),
                    &mut io::sink(),
                )
                .map_err(|e| format!("跳过已满足的前置组件流 {} 失败: {}", entry.relative_path, e))?;
            }
            bytes_done += entry.uncompressed_size;
            continue;
        }

        let clean_rel: PathBuf = entry
            .relative_path
            .split(['\\', '/'])
            .filter(|seg| !seg.is_empty() && *seg != "." && *seg != "..")
            .collect();
        let dest_path = target_install_dir.join(&clean_rel);

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("创建子目录 {} 失败: {}", parent.display(), e))?;
        }

        let mut raw_buf = vec![0u8; entry.uncompressed_size as usize];
        lzma_reader
            .read_exact(&mut raw_buf)
            .map_err(|e| format!("从 LZMA2 固实流解压文件 {} 失败: {}", entry.relative_path, e))?;

        // Reverse x86 BCJ pre-filter if applied during packaging
        if entry.bcj_filtered {
            bcj_x86_decode(&mut raw_buf);
        }

        let mut hasher = Hasher::new();
        hasher.update(&raw_buf);
        let actual_crc = hasher.finalize();
        if actual_crc != entry.crc32 {
            return Err(format!(
                "文件 {} 完整性校验失败 (CRC32 不匹配: 期望 {:08X}, 实际 {:08X})",
                entry.relative_path, entry.crc32, actual_crc
            ));
        }

        let mut create_res = File::create(&dest_path);
        if create_res.is_err() && kill_locked_processes {
            for _ in 0..6 {
                let killed = crate::win32_ops::terminate_running_target_processes(
                    target_install_dir,
                    &candidate_files,
                );
                if let Some(tx) = progress_tx {
                    for msg in killed {
                        let _ = tx.send(InstallProgressEvent::KillingProcesses(msg));
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(120));
                create_res = File::create(&dest_path);
                if create_res.is_ok() {
                    break;
                }
            }
        }

        let mut out_f = create_res.map_err(|e| {
            if e.raw_os_error() == Some(32) || e.raw_os_error() == Some(5) {
                format!(
                    "目标文件被正在运行的进程占用 ({})，请勾选“自动结束运行中的进程”或点击下方“结束进程并重试”。({})",
                    entry.relative_path, e
                )
            } else {
                format!("无法写入目标文件 {}: {}", entry.relative_path, e)
            }
        })?;
        out_f
            .write_all(&raw_buf)
            .map_err(|e| format!("写入文件 {} 失败: {}", entry.relative_path, e))?;

        bytes_done += entry.uncompressed_size;
        installed_files.push(clean_rel.to_string_lossy().to_string());
    }

    // 2. Execute unsatisfied Prerequisite actions (RunBundledExe & RunCommand)
    for (rule_idx, rule) in manifest.config.prerequisites.iter().enumerate() {
        if !rule.enabled {
            continue;
        }
        match rule.action {
            PrereqAction::RunBundledExe {
                ref args,
                elevate,
                wait,
                ref success_exit_codes,
                ..
            } => {
                if let Some((_, _, ref temp_exe)) = needed_bundled_prereqs
                    .iter()
                    .find(|(r_idx, _, _)| *r_idx == rule_idx)
                {
                    if let Some(tx) = progress_tx {
                        let _ = tx.send(InstallProgressEvent::RunningPrerequisite(rule.name.clone()));
                    }
                    let exec_res = crate::win32_ops::execute_prereq_command(
                        &rule.name,
                        &temp_exe.to_string_lossy(),
                        args,
                        elevate,
                        wait,
                        success_exit_codes,
                        Some(target_install_dir),
                    );
                    let _ = fs::remove_file(temp_exe);
                    exec_res?;
                }
            }
            PrereqAction::RunCommand {
                ref program,
                ref args,
                elevate,
                wait,
                ref success_exit_codes,
            } => {
                let passed = crate::win32_ops::evaluate_prereq_condition(
                    &rule.condition,
                    Some(target_install_dir),
                );
                if !passed {
                    if let Some(tx) = progress_tx {
                        let _ = tx.send(InstallProgressEvent::RunningPrerequisite(rule.name.clone()));
                    }
                    crate::win32_ops::execute_prereq_command(
                        &rule.name,
                        program,
                        args,
                        elevate,
                        wait,
                        success_exit_codes,
                        Some(target_install_dir),
                    )?;
                }
            }
            PrereqAction::PromptAndAbort { .. } => {}
        }
    }

    Ok(installed_files)
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bcj_and_solid_lzma2_roundtrip() {
        let mut sample_pe = b"MZ\x90\x00".to_vec();
        for i in 0..5000u32 {
            sample_pe.push(0xE8);
            let rel = (0x1000u32).wrapping_sub(i * 5);
            sample_pe.extend_from_slice(&rel.to_le_bytes());
        }
        let original = sample_pe.clone();
        bcj_x86_encode(&mut sample_pe);
        assert_ne!(sample_pe, original);
        bcj_x86_decode(&mut sample_pe);
        assert_eq!(sample_pe, original);

        let (compressed, dict) = lzma2_compress_bytes(&original, 6).expect("compress");
        let decompressed = lzma2_decompress_bytes(&compressed, dict).expect("decompress");
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_built_demo_installer_extraction() {
        let setup_exe = Path::new("dist/NyanStudioApp_Setup.exe");
        if setup_exe.exists() {
            let (_footer, manifest) = read_package_from_exe(setup_exe)
                .expect("read_package_from_exe IO")
                .expect("valid NYANPK02 footer");
            println!(
                "Manifest files: {}, uncompressed: {}, compressed: {}, dict: {}",
                manifest.files.len(),
                manifest.total_uncompressed_bytes,
                manifest.total_compressed_bytes,
                manifest.dict_size
            );
            let out_dir = Path::new("dist/test_extract_out");
            let extracted =
                extract_package_files(setup_exe, &manifest, out_dir, false, None).expect("extract_package_files");
            assert_eq!(extracted.len(), manifest.files.len());
            let _ = fs::remove_dir_all(out_dir);
        }
    }

    #[test]
    fn test_custom_installer_and_uninstaller_icon() {
        let current_exe = std::env::current_exe().expect("current exe");
        let temp_dir = std::env::temp_dir().join(format!("nyan_test_custom_ico_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        let src_dir = temp_dir.join("src");
        let _ = fs::create_dir_all(&src_dir);
        fs::write(src_dir.join("app.txt"), b"Hello Nyan World!").unwrap();

        // Create a custom 16x16 red icon
        let custom_ico = crate::icon::tests::make_test_ico(0, 0, 255);
        let custom_ico_path = temp_dir.join("custom.ico");
        fs::write(&custom_ico_path, &custom_ico).unwrap();

        let out_installer = temp_dir.join("MyCustomSetup.exe");
        let mut cfg = PackageConfig::default();
        cfg.installer_icon = Some(custom_ico_path.to_string_lossy().to_string());
        cfg.main_exe = "app.txt".to_string();

        let res = build_installer_package(&current_exe, &src_dir, &out_installer, &cfg, None);
        assert!(res.is_ok(), "build_installer_package with custom icon failed: {:?}", res);

        // 1. Verify installer PE has the custom icon in its resources
        let (group_inst, first_inst, _) = crate::icon::tests::read_icon_resources(&out_installer);
        let custom_frames = crate::icon::parse_ico(&custom_ico).unwrap();
        assert_eq!(u16::from_le_bytes([group_inst[4], group_inst[5]]), 1, "installer has 1 icon frame in group");
        assert_eq!(first_inst, custom_frames[0].data, "installer RT_ICON equals custom icon frame");

        // 2. Simulate install: generate uninstaller using extract_pure_stub_bytes (same as write_uninstaller_files)
        let unins_exe = temp_dir.join("unins000.exe");
        let stub_bytes = extract_pure_stub_bytes(&out_installer).expect("extract stub from custom installer");
        fs::write(&unins_exe, &stub_bytes).unwrap();

        // 3. Verify uninstaller PE has the exact same custom icon in its resources
        let (group_unins, first_unins, _) = crate::icon::tests::read_icon_resources(&unins_exe);
        assert_eq!(u16::from_le_bytes([group_unins[4], group_unins[5]]), 1, "uninstaller has 1 icon frame in group");
        assert_eq!(first_unins, custom_frames[0].data, "uninstaller RT_ICON equals custom icon frame");

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_bundled_prerequisite_packaging_and_conditional_execution() {
        let current_exe = std::env::current_exe().expect("current exe");
        let temp_dir =
            std::env::temp_dir().join(format!("nyan_test_prereq_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        let src_dir = temp_dir.join("src");
        let _ = fs::create_dir_all(&src_dir);
        fs::write(src_dir.join("app.txt"), b"Main Application Payload").unwrap();

        // Create a dummy bundled prerequisite executable (use cmd.exe copy so it can run with /C)
        let sys_cmd = PathBuf::from(
            std::env::var("COMSPEC").unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".to_string()),
        );
        let mock_redist = temp_dir.join("mock_vc_redist.exe");
        fs::copy(&sys_cmd, &mock_redist).expect("copy cmd.exe as mock_vc_redist.exe");

        let out_installer = temp_dir.join("PrereqSetup.exe");
        let mut cfg = PackageConfig::default();
        cfg.main_exe = "app.txt".to_string();

        // Rule 1: Condition that is ALREADY SATISFIED (kernel32.dll exists) -> should NOT run
        let satisfied_marker = temp_dir.join("should_not_exist.txt");
        cfg.prerequisites.push(PrerequisiteRule {
            name: "Satisfied Rule".to_string(),
            enabled: true,
            condition: PrereqCondition::File {
                path: "%SystemRoot%\\System32\\kernel32.dll".to_string(),
                operator: CheckOperator::Exists,
                expected_version: String::new(),
            },
            action: PrereqAction::RunBundledExe {
                source_file: mock_redist.to_string_lossy().to_string(),
                bundled_rel_path: String::new(),
                args: format!("/C echo skip > \"{}\"", satisfied_marker.display()),
                elevate: false,
                wait: true,
                success_exit_codes: vec![0],
            },
        });

        // Rule 2: Condition that is UNSATISFIED (non-existent registry key) -> SHOULD extract to %TEMP%, run, and clean up %TEMP%
        let unsatisfied_marker = temp_dir.join("prereq_executed_ok.txt");
        cfg.prerequisites.push(PrerequisiteRule {
            name: "Unsatisfied VC Runtime Mock".to_string(),
            enabled: true,
            condition: PrereqCondition::Registry {
                root: RegHiveRoot::Hkcu,
                subkey: "Software\\NyanNonExistentRuntimeKey_99999".to_string(),
                value_name: "Installed".to_string(),
                arch_view: RegArchView::Default,
                operator: CheckOperator::Equals,
                expected: "1".to_string(),
            },
            action: PrereqAction::RunBundledExe {
                source_file: mock_redist.to_string_lossy().to_string(),
                bundled_rel_path: String::new(),
                args: format!("/C echo installed > \"{}\"", unsatisfied_marker.display()),
                elevate: false,
                wait: true,
                success_exit_codes: vec![0],
            },
        });

        build_installer_package(&current_exe, &src_dir, &out_installer, &cfg, None)
            .expect("build installer with bundled prerequisites");

        let (_footer, manifest) = read_package_from_exe(&out_installer)
            .expect("read io")
            .expect("manifest");

        // Package manifest has 1 normal file + 2 bundled prerequisite entries
        assert_eq!(manifest.files.len(), 3);

        let install_target = temp_dir.join("installed_app");
        let installed_files =
            extract_package_files(&out_installer, &manifest, &install_target, false, None)
                .expect("extract and run prereqs");

        // Only `app.txt` is recorded in `installed_files` (bundled prereqs are excluded from uninstaller receipt)
        assert_eq!(installed_files, vec!["app.txt".to_string()]);
        assert!(install_target.join("app.txt").exists());

        // Rule 1 was satisfied -> marker must NOT exist
        assert!(
            !satisfied_marker.exists(),
            "Satisfied prerequisite should be skipped!"
        );
        // Rule 2 was unsatisfied -> marker MUST exist
        assert!(
            unsatisfied_marker.exists(),
            "Unsatisfied bundled prerequisite should have executed!"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
