# Nyan Setup

基于 Rust 与 Win32 Direct2D 的轻量级 Windows 软件安装与打包工具。采用静态链接，无运行时依赖，提供可视化设计器（Studio）与命令行打包工具，生成单文件自解压安装包。

## 特性

- **零外部依赖**：基于 Rust 与 Win32 原生 API 开发，采用静态 CRT 链接（`+crt-static`），无需安装 Visual C++ 运行库或 .NET 环境。
- **现代化图形界面**：基于 Direct2D 与 DirectWrite 硬件加速渲染，支持 Fluent 风格设计与暗色主题。
- **高压缩率归档**：内置 x86 BCJ 指令预处理与固实 Solid LZMA2 字典压缩，有效缩减安装包体积。
- **单执行文件架构**：安装包采用自解压 Stub + 固实压缩流 + 尾部清单架构，单个可执行文件即可独立运行。
- **PE 资源动态注入**：支持打包时指定自定义 `.ico` 图标，自动通过 Windows UpdateResource API 重构替换 PE 内部图标资源。
- **深度卸载清理**：
  - 自动检测并安全终止安装目录内运行的目标进程；
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

使用 `pack` 子命令通过 JSON 配置文件进行自动化打包：

```powershell
# 使用默认图标打包
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
  "create_start_menu_shortcut": true
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
├── resources/            # 应用程序资源
│   ├── app.ico           # 多尺寸图标文件
│   ├── app.rc            # Windows PE 资源脚本
│   └── icons/            # SVG 矢量源及各尺寸 PNG 资源
├── src/
│   ├── main.rs           # 程序入口与 CLI 分发
│   ├── d2d.rs            # Direct2D / DirectWrite UI 渲染引擎
│   ├── icon.rs           # ICO 格式解析与 PE 资源动态替换
│   ├── package.rs        # Solid LZMA2 + BCJ 压缩归档引擎
│   ├── gui_installer.rs  # 安装器向导与卸载器向导 UI
│   ├── gui_studio.rs     # 可视化 Studio 打包设计器
│   └── win32_ops.rs      # Win32 系统接口、进程管理与注册表清理
├── build.rs              # Windows 资源编译脚本
└── Cargo.toml            # 项目配置与依赖定义
```

## 许可证

本项目采用 MIT 许可证。详情请参阅相关授权说明。
