# Nyan Setup

基于 Rust 与 Win32 Direct2D 的轻量级 Windows 软件安装与打包工具。采用静态链接，无运行时依赖，提供可视化设计器（Studio）与命令行打包工具，生成单文件自解压安装包。

## 特性

- **零外部依赖**：基于 Rust 与 Win32 原生 API 开发，采用静态 CRT 链接（`+crt-static`），无需安装 Visual C++ 运行库或 .NET 环境。
- **现代化图形界面**：基于 Direct2D 与 DirectWrite 硬件加速渲染，支持 Fluent 风格设计与暗色主题。
- **高压缩率归档**：内置 x86 BCJ 指令预处理与固实 Solid LZMA2 字典压缩，有效缩减安装包体积。
- **单执行文件架构**：安装包采用自解压 Stub + 固实压缩流 + 尾部清单架构，单个可执行文件即可独立运行。
- **PE 资源动态注入**：支持打包时指定自定义 `.ico` 图标，自动通过 Windows UpdateResource API 重构替换 PE 内部图标资源。
- **前置环境检测与部署**：支持在打包时声明注册表键值检测（`HKLM` / `HKCU` / `HKCR` / `HKU`、32/64 位视图）或系统文件及 PE 版本（`VS_FIXEDFILEINFO`）校验规则；条件不满足时可自动解压并静默执行随包附带的运行库（如 `vc_redist.x64.exe`）、运行自定义命令或弹窗提示。
- **进程冲突检测与 Windows 系统服务联动**：
  - 集成 Windows Restart Manager (`Rstrtmgr.dll`) 与句柄锁检测，精准定位占用目标目录 `.exe` / `.dll` 的进程与服务，支持打包配置 `auto_kill`（自动结束占用进程）或 `prompt_user`（弹出交互式占用提示窗）；
  - **Windows 服务全生命周期管理**：自动扫描 `HKLM\SYSTEM\CurrentControlSet\Services` 中 `ImagePath`（及 NSSM `Parameters\Application`）指向安装目录的系统服务，并支持在配置中显式声明 `services`；覆盖更新时自动停止运行中的服务、覆写文件后恢复启动（当前账户属于管理员组时，停止与恢复在同一个提权进程内完成，只请求一次 UAC）；卸载时自动停止服务并调用 `DeleteService` 彻底注销，非管理员运行时支持按需请求 UAC 提权。
- **深度卸载清理**：
  - 自动停止并删除关联系统服务，安全终止安装目录内运行的目标进程；
  - 递归清理文件与空子目录，结合安全目录校验机制自销毁安装根目录；
  - 彻底清理系统注册表残留，覆盖应用卸载项、App Paths、开始菜单磁贴属性树（TileProperties）、音量合成器缓存（Audio PolicyConfig）、MuiCache、自启动与启动任务管理项等。

## 运行环境与依赖要求

- 操作系统：Windows 10 / Windows 11（x86_64）
- 构建工具：Rust 1.75+（需 MSVC 工具链及 Windows SDK）

## 编译与构建

```powershell
# 编译 Release 版本
cargo build --release

# 运行全套单元与集成测试
cargo test --release
```

## 使用说明

### 1. 可视化打包设计器（Studio）

直接运行生成的二进制文件，将启动可视化设计器界面：

```powershell
.\target\release\nyan-setup.exe
```

在 Studio 中可配置应用名称、版本号、默认安装路径、桌面与开始菜单快捷方式、可执行程序图标，并实时构建输出安装包。

### 2. 命令行打包

使用 `init-config` 生成模板配置，并通过 `pack` 子命令执行自动化打包（详细字段说明请参考 [命令行配置文件打包操作文档](docs/CLI_PACKAGING_GUIDE.md)）：

```powershell
# 生成模板配置文件 package.json
nyan-setup.exe init-config package.json

# 使用配置文件打包
nyan-setup.exe pack --config package.json

# 指定自定义图标打包
nyan-setup.exe pack --config package.json --icon resources\app.ico
```

配置文件示例（`package.json`）：

```json
{
  "app_name": "示例应用程序",
  "app_id": "com.example.app",
  "app_version": "1.0.0",
  "publisher": "Example Team",
  "main_exe": "app.exe",
  "default_install_dir": "%LOCALAPPDATA%\\Programs\\ExampleApp",
  "source_dir": "./build_output",
  "output_exe": "./dist/ExampleApp_Setup.exe",
  "create_desktop_shortcut": true,
  "create_start_menu_shortcut": true,
  "process_conflict_policy": "auto_kill",
  "services": [],
  "prerequisites": [
    {
      "name": "Microsoft VC++ 2015-2022 运行库 (x64)",
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

### 3. 构建演示示例

工程内置演示程序生成器，可一键生成示例源码并打包出独立安装包：

```powershell
cargo run --release -- build-demo
```

构建产物将输出至 `dist/NyanStudioApp_Setup.exe`。

## 项目结构

```text
Nyan-setup/
├── .cargo/               # 编译器静态链接配置 (+crt-static)
├── docs/                 # 详细打包与命令行配置操作指南
├── resources/            # 应用程序图标与 PE 资源脚本
├── src/
│   ├── main.rs           # 程序主入口与运行模式路由
│   ├── cli.rs            # 命令行子命令实现 (init-config, pack, build-demo)
│   ├── icon.rs           # ICO 格式解析与 PE 资源动态替换
│   ├── d2d/              # Direct2D / DirectWrite 渲染引擎、配色与通用控件
│   ├── package/          # Solid LZMA2 + BCJ 固实压缩与流式解包引擎
│   ├── gui_installer/    # 安装器向导与卸载器向导 GUI
│   ├── gui_studio/       # 可视化 Studio 打包设计器 GUI
│   └── win32_ops/        # Win32 系统接口、SCM 服务管理、进程检测与注册表清理
├── build.rs              # Windows 资源编译脚本
└── Cargo.toml            # 项目配置与依赖定义
```

## 许可证

本项目采用 MIT 许可证。详情请参阅相关授权说明。
