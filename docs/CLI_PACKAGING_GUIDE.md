# Nyan Setup 命令行与配置文件打包指南 (CLI Packaging Guide)

本文档详细介绍如何使用 `nyan-setup.exe` 的命令行模式与 JSON 配置文件（`package.json`）完成自动化安装包构建、前置环境检测配置、自定义图标注入以及无界面验证。

---

## 一、命令行子命令速览

`nyan-setup.exe` 支持以下命令行操作模式：

| 命令格式 | 说明 |
| :--- | :--- |
| `nyan-setup.exe` | 不带参数启动可视化打包工具 **Nyan Setup Studio** |
| `nyan-setup.exe init-config [输出路径.json]` | 生成一份带完整默认值的 `package.json` 配置文件模板（默认输出到当前目录 `package.json`） |
| `nyan-setup.exe pack --config <package.json> [覆盖参数]` | **（推荐）基于 JSON 配置文件打包**，支持 `--source`、`--output`、`--icon` 覆盖对应配置项 |
| `nyan-setup.exe pack <package.json>` | `--config` 的简写形式，直接传入 `.json` 配置文件路径即可打包 |
| `nyan-setup.exe pack <源目录> <输出.exe> [应用名] [版本] [主程序] [--icon <图标.ico>]` | 无需配置文件的快速命令行打包模式 |
| `nyan-setup.exe build-demo [输出.exe]` | 自动生成演示应用目录并构建示例安装包 |
| `<生成的安装包.exe> --extract-to <目标目录>` | 对已生成的安装包执行命令行静默解压并校验全量文件 `CRC32`（适用于 CI/CD 自动化验证） |

---

## 二、快速上手流程

### 第 1 步：生成配置文件模板

在 PowerShell 或 CMD 中执行：

```powershell
.\nyan-setup.exe init-config package.json
```

该命令会在当前目录生成一份结构完整的 `package.json` 模板文件。

### 第 2 步：准备应用文件目录

假设你的待发布程序目录结构如下：

```text
my-project/
├── package.json              # 打包配置文件
├── assets/
│   └── app.ico               # 自定义安装包/卸载程序图标（可选）
├── redist/
│   └── vc_redist.x64.exe     # 随包附带的前置运行库安装器（可选）
└── build_output/             # 待打包的程序源目录 (source_dir)
    ├── MyApp.exe             # 主程序入口 (main_exe)
    ├── core.dll
    └── config/
        └── settings.toml
```

### 第 3 步：执行命令行打包

```powershell
# 方式 1：使用 --config 指定配置文件
.\nyan-setup.exe pack --config package.json

# 方式 2：简写形式（直接传入 .json 文件）
.\nyan-setup.exe pack package.json

# 方式 3：在 CI/CD 中通过命令行参数动态覆盖源目录、输出路径或图标
.\nyan-setup.exe pack --config package.json --source .\build_output --output .\dist\MyApp_v1.2.0_Setup.exe --icon .\assets\app.ico
```

打包成功后，控制台将输出类似信息：

```text
[SUCCESS] Packed 12 files (18.4 MB) -> .\dist\MyApp_Setup.exe (4.2 MB)
```

---

## 三、`package.json` 配置文件完整字段参考

配置文件采用标准 JSON 格式。除 `source_dir` 和 `output_exe`（若未通过命令行 `--source` / `--output` 传入）外，其余字段若省略均会自动回退至内置默认值。

### 3.1 完整配置示例

```json
{
  "source_dir": "./build_output",
  "output_exe": "./dist/MyApp_Setup.exe",
  "app_id": "com.mycompany.myapp",
  "app_name": "My Application",
  "app_version": "1.2.0",
  "publisher": "MyCompany Inc.",
  "main_exe": "MyApp.exe",
  "default_install_dir": "%LOCALAPPDATA%\\Programs\\My Application",
  "installer_icon": "./assets/app.ico",
  "compression_level": 6,
  "create_desktop_shortcut": true,
  "create_start_menu_shortcut": true,
  "register_uninstaller": true,
  "auto_launch_after_install": true,
  "process_conflict_policy": "prompt_user",
  "services": ["MyAppDaemonSvc"],
  "license_text": "终端用户软件许可协议 (EULA)\n\n1. 许可授予\n...",
  "prerequisites": [
    {
      "name": "VC++ 2015-2022 运行库 (x64)",
      "enabled": true,
      "condition": {
        "kind": "registry",
        "root": "hklm",
        "subkey": "SOFTWARE\\Microsoft\\VisualStudio\\14.0\\VC\\Runtimes\\x64",
        "value_name": "Installed",
        "arch_view": "x64",
        "operator": "equals",
        "expected": "1"
      },
      "action": {
        "kind": "run_bundled_exe",
        "source_file": "./redist/vc_redist.x64.exe",
        "args": "/install /quiet /norestart",
        "elevate": true,
        "wait": true,
        "success_exit_codes": [0, 1638, 3010]
      }
    }
  ]
}
```

### 3.2 顶层基础字段说明

| 字段名 | 类型 | 必填 | 默认值 | 说明 |
| :--- | :--- | :---: | :--- | :--- |
| `source_dir` | `string` | 是* | — | 待打包的源文件夹路径（支持相对 `package.json` 所在目录的路径）。也可通过命令行 `--source` 传入。 |
| `output_exe` | `string` | 是* | — | 输出的单文件安装包 `.exe` 路径。也可通过命令行 `--output` 传入。 |
| `app_id` | `string` | 否 | `"com.nyansoft.demoapp"` | 应用全局唯一标识符，用作注册表 `Uninstall\<app_id>` 键名及覆盖更新检测标识。建议使用反向域名或 GUID。 |
| `app_name` | `string` | 否 | `"Nyan Studio App"` | 应用展示名称（用于向导界面标题、快捷方式名称及“应用和功能”列表显示名称）。 |
| `app_version` | `string` | 否 | `"1.0.0"` | 应用版本号（如 `"1.2.0"`）。 |
| `publisher` | `string` | 否 | `"Nyan Softworks Inc."` | 发布者/公司名称（写入系统“应用和功能”卸载信息）。 |
| `main_exe` | `string` | 否 | `"NyanApp.exe"` | 主程序相对于 `source_dir` 的路径（如 `"MyApp.exe"` 或 `"bin\\MyApp.exe"`），用于创建快捷方式、提取应用图标及安装完成后立即启动。 |
| `default_install_dir` | `string` | 否 | `"%LOCALAPPDATA%\\Programs\\<app_name>"` | 默认安装路径。支持环境变量展开：`%LOCALAPPDATA%`、`%APPDATA%`、`%PROGRAMFILES%`、`%PROGRAMFILES(X86)%`、`%USERPROFILE%`、`%TEMP%`。 |
| `installer_icon` | `string \| null` | 否 | `null` | 自定义 `.ico` 图标文件路径（支持 32bpp DIB / PNG 帧格式）。留空 `null` 时使用内置原创 Q 版二次元赛璐璐多分辨率图标（`resources/app.ico`，16–256px 共 7 帧）；指定后将在打包时直接替换安装包 `.exe` 与卸载器 `unins000.exe` 的 PE 原生图标资源（`RT_GROUP_ICON` / `RT_ICON`），并同步生效于 Direct2D 标题栏与向导品牌卡片。 |
| `compression_level` | `number` | 否 | `6` | 固实 LZMA2 压缩等级，取值范围 `1` ~ `9`（数值越大压缩率越高、字典越大，推荐 `6` ~ `9`）。 |
| `create_desktop_shortcut` | `boolean` | 否 | `true` | 安装向导中“在桌面创建快捷方式”选项的默认勾选状态。 |
| `create_start_menu_shortcut` | `boolean` | 否 | `true` | 安装向导中“在开始菜单创建快捷方式”选项的默认勾选状态。 |
| `register_uninstaller` | `boolean` | 否 | `true` | 是否默认生成 `unins000.exe` 并注册到 Windows“应用和功能”列表。 |
| `auto_launch_after_install` | `boolean` | 否 | `true` | 安装完成页中“立即运行主程序”选项的默认勾选状态。 |
| `process_conflict_policy` | `string` | 否 | `"auto_kill"` | **安装/更新时检测到目标目录文件被进程占用时的处理策略**：<br>• `"auto_kill"`：安装前自动停止关联服务、终止占用进程并继续安装，更新后自动恢复服务；<br>• `"prompt_user"`：弹出模态提示窗口列出占用进程名与 `PID`（及服务标识），由用户选择「重试」或「结束进程并重试」。 |
| `services` | `array<string>` | 否 | `[]` | **关联的 Windows 系统服务名称列表**（如 `["MyAppService"]`）。<br>• **零配置自动识别**：即使留空 `[]`，在覆盖更新和卸载时也会通过注册表 `Services\<Name>\ImagePath`（含 NSSM `Parameters\Application`）及 Windows Restart Manager (`RmService`) 自动发现所有归属安装目录的系统服务；<br>• **更新时**：自动停止运行中的关联服务 → 覆盖文件 → 自动恢复启动服务；<br>• **卸载时**：自动停止服务并等待 `SERVICE_STOPPED` → 调用 `DeleteService` 注销删除服务（非管理员运行遇系统服务时将按需请求 UAC 提权）。 |
| `license_text` | `string` | 否 | 内置标准 EULA | 安装向导第 1 步展示的《软件许可协议》正文（支持 `\n` 换行）。 |
| `prerequisites` | `array` | 否 | `[]` | 前置运行环境检测与自动部署规则列表（详见第四节）。 |

---

## 四、前置运行环境检测规则 (`prerequisites`) 详解

`prerequisites` 数组中的每一项代表一条前置环境检测规则，在安装阶段按顺序执行。每条规则包含以下字段：

```json
{
  "name": "规则显示名称",
  "enabled": true,
  "condition": { ... },
  "action": { ... }
}
```

### 4.1 检测条件 (`condition`)

`condition` 通过 `"kind"` 字段区分两种检测模式：

#### 模式 1：注册表键值检测 (`"kind": "registry"`)

```json
"condition": {
  "kind": "registry",
  "root": "hklm",
  "subkey": "SOFTWARE\\Microsoft\\VisualStudio\\14.0\\VC\\Runtimes\\x64",
  "value_name": "Installed",
  "arch_view": "x64",
  "operator": "equals",
  "expected": "1"
}
```

| 字段 | 可选值 | 默认值 | 说明 |
| :--- | :--- | :--- | :--- |
| `root` | `"hklm"` / `"hkcu"` / `"hkcr"` / `"hku"` | `"hklm"` | 注册表根键（分别对应 `HKEY_LOCAL_MACHINE`、`HKEY_CURRENT_USER`、`HKEY_CLASSES_ROOT`、`HKEY_USERS`） |
| `subkey` | `string` | — | 注册表子键路径（不含根键前缀） |
| `value_name` | `string` | `""` | 注册表值名称。留空 `""` 时仅检测该注册表子键是否存在（或读取默认未命名值） |
| `arch_view` | `"default"` / `"x64"` / `"x86"` | `"x64"` | 注册表 32/64 位视图（`"x64"` 对应 `KEY_WOW64_64KEY`，`"x86"` 对应 `KEY_WOW64_32KEY`） |
| `operator` | `"exists"` / `"not_exists"` / `"equals"` / `"not_equals"` / `"greater_or_equal"` / `"contains"` | `"equals"` | 比较运算符（其中 `"greater_or_equal"` 支持多段版本号如 `14.38.33130.0` 或整数数值对比） |
| `expected` | `string` | `""` | 期望的目标值或最低版本号（当 `operator` 为 `"exists"` / `"not_exists"` 时可留空） |

#### 模式 2：系统文件或 PE 版本检测 (`"kind": "file"`)

```json
"condition": {
  "kind": "file",
  "path": "%SystemRoot%\\System32\\vcruntime140.dll",
  "operator": "greater_or_equal",
  "expected_version": "14.38.0.0"
}
```

| 字段 | 可选值 | 默认值 | 说明 |
| :--- | :--- | :--- | :--- |
| `path` | `string` | — | 目标文件路径。支持 `%SystemRoot%`、`%WINDIR%`、`%PROGRAMFILES%`、`%PROGRAMFILES(X86)%`、`%LOCALAPPDATA%`、`%APPDATA%`、`%TEMP%` 以及代表当前目标安装目录的 `%INSTALL_DIR%` |
| `operator` | `"exists"` / `"not_exists"` / `"greater_or_equal"` / `"equals"` | `"exists"` | 检测方式：`"exists"` 仅判断文件是否存在；`"greater_or_equal"` 与 `"equals"` 会读取文件 PE 资源中的 `VS_FIXEDFILEINFO` 版本号进行对比 |
| `expected_version` | `string` | `""` | 期望的 PE 文件版本号（如 `"14.38.0.0"`，仅检测存在时可留空） |

---

### 4.2 条件未满足时的执行动作 (`action`)

当 `condition` 检测未通过时，安装器将执行 `action` 定义的逻辑。通过 `"kind"` 字段区分三种动作类型：

#### 动作 1：随包打包并运行安装器 (`"kind": "run_bundled_exe"`)

在打包构建时自动将指定的运行库安装程序（如 `vc_redist.x64.exe`）压缩进固实归档流；安装时仅在目标机器检测未通过时才将其解压至 `%TEMP%` 目录执行，执行完毕后自动删除临时文件。

```json
"action": {
  "kind": "run_bundled_exe",
  "source_file": "./redist/vc_redist.x64.exe",
  "args": "/install /quiet /norestart",
  "elevate": true,
  "wait": true,
  "success_exit_codes": [0, 1638, 3010]
}
```

- `source_file`：构建机器上的安装器文件路径（支持绝对路径、相对 `package.json` 的路径或相对 `source_dir` 的路径）。
- `args`：传递给该安装器的命令行参数（例如 VC++ 运行库静默安装参数 `"/install /quiet /norestart"`）。
- `elevate`：是否请求 UAC 管理员提权运行（`true` 使用 `"runas"` 动词）。
- `wait`：是否等待该进程执行完成后再继续后续安装步骤（默认 `true`）。
- `success_exit_codes`：视为执行成功的进程退出码列表。默认 `[0, 1638, 3010]`（其中 `0` 为成功，`1638` 为已安装更高版本，`3010` 为成功但提示重启）。

#### 动作 2：执行系统命令或安装目录脚本 (`"kind": "run_command"`)

不额外捆绑外部文件，直接调用系统命令（如 `powershell.exe`、`cmd.exe`、`regsvr32.exe`）或刚释放到 `%INSTALL_DIR%` 下的程序。

```json
"action": {
  "kind": "run_command",
  "program": "%INSTALL_DIR%\\tools\\init_env.exe",
  "args": "--silent --register",
  "elevate": true,
  "wait": true,
  "success_exit_codes": [0]
}
```

#### 动作 3：弹窗提示用户并中止安装 (`"kind": "prompt_and_abort"`)

在开始解压文件前进行前置拦截。若条件不满足，则弹出提示框展示 `message`；若配置了 `download_url`，将询问用户是否立即调用默认浏览器打开该下载页面，并安全中止安装。

```json
"action": {
  "kind": "prompt_and_abort",
  "message": "系统缺少 Microsoft Edge WebView2 运行时，请先安装后再运行本安装程序。",
  "download_url": "https://developer.microsoft.com/microsoft-edge/webview2/"
}
```

---

## 五、Windows 系统服务自动管理与配置说明

当应用程序包含 Windows 后台系统服务（無論是在安装阶段通过 `prerequisites` 的 `run_command` 注册，还是应用程序运行后自行向系统注册）时，Nyan Setup 采用**“零配置自动识别 + 可选显式声明 + 按需 UAC 提权”**机制：

### 5.1 零配置自动识别机制（推荐）

即使在 `package.json` 中省略 `services`（或保持 `"services": []`），安装器与卸载器也会通过以下双重机制自动发现归属当前应用的系统服务：

1. **注册表 `Services` 路径反查**：
   在通过安全应用目录校验的前提下，自动扫描 `HKLM\SYSTEM\CurrentControlSet\Services` 下的 Win32 服务（排除底层内核驱动），凡是 `ImagePath`（或 NSSM / `srvany` 包装器的 `Parameters\Application`、`Parameters\AppDirectory`）指向当前安装目录 `install_dir` 内部的服务，均会被自动识别。
2. **Windows Restart Manager (`Rstrtmgr.dll`) 实时锁检测**：
   当目录下的 `.exe` 或 `.dll` 被运行中的服务锁定时，Restart Manager 会直接识别出 `RmService` 并提取其系统服务短名称（`strServiceShortName`）。

### 5.2 显式声明服务名称（可选兜底）

若您的服务托管在系统公共宿主（如 `svchost.exe`）中且注册表路径未指向应用安装目录，可在 `package.json` 顶层显式声明服务名列表：

```json
"services": ["MyAppDaemonSvc", "MyAppUpdaterSvc"]
```

### 5.3 覆盖更新与卸载时的执行流程

- **覆盖更新（Update）**：
  1. **更新前**：自动识别关联服务并记录当前处于运行状态的服务列表，随后向 SCM 发送 `SERVICE_CONTROL_STOP` 并轮询等待状态进入 `SERVICE_STOPPED`（彻底释放文件占用锁）；
  2. **更新中**：解压并覆写新版文件；
  3. **更新后**：自动调用 `StartServiceW` 将更新前正在运行的服务恢复启动。
- **卸载清理（Uninstall）**：
  1. 自动汇总卸载清单（`unins000.dat`）记录的服务与实时扫描安装目录发现的服务；
  2. 先停止服务并等待完全退出（`SERVICE_STOPPED`），再调用 Win32 `DeleteService` 将服务从 SCM 中彻底注销，并清理残留注册表项；
  3. 最后执行常规进程清理与安装目录删除。
- **按需静默 UAC 提权**：
  普通无服务应用全程无需 UAC 提权。覆盖更新启动时，如果检测到有关联服务正在运行且当前 Windows 账户属于管理员组，安装器会先通过 `runas` 重新启动自身并只请求一次 UAC；停止服务、覆写文件和恢复启动都在这个已提权进程内完成，不会再弹第二次。全新安装、没有运行中服务的更新，以及使用标准用户账户时不会做整体提权（标准用户仍按原方式按需请求管理员凭据）。卸载器在删除关联服务时仍按需请求一次 UAC，无需关闭或重启当前向导窗口。

---

## 六、CI/CD 自动化构建与校验示例

在 GitHub Actions 或本地自动化构建脚本（PowerShell）中，可以结合 `pack --config` 与 `--extract-to` 实现从打包到完整性验证的全流程自动化：

```powershell
$ErrorActionPreference = "Stop"

# 1. 使用配置文件构建单文件安装包
.\target\release\nyan-setup.exe pack --config .\package.json --output .\dist\MyApp_Setup.exe

# 2. 命令行无界面解压并执行全量文件 CRC32 校验
$VerifyDir = Join-Path $env:TEMP "nyan_verify_test"
if (Test-Path $VerifyDir) { Remove-Item -Recurse -Force $VerifyDir }

.\dist\MyApp_Setup.exe --extract-to $VerifyDir

# 3. 清理临时校验目录
Remove-Item -Recurse -Force $VerifyDir
Write-Host "安装包构建与 CRC32 校验全部通过！"
```
