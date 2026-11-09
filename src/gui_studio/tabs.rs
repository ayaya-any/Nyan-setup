use super::{FocusedField, StudioTab, StudioWindowState};
use crate::d2d::{rgb, rgba, ButtonVariant, D2DRenderer, RectF, VectorIcon};
use crate::package::{format_bytes, ProcessConflictPolicy};
use windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE;
use windows::Win32::UI::HiDpi::GetDpiForWindow;

impl StudioWindowState {
    pub(crate) fn draw_card_header(&self, card: RectF, title: &str, subtitle: &str) {
        let pal = self.palette;
        let x0 = card.x + 28.0;
        let w_full = card.w - 56.0;

        self.renderer.draw_text(
            title,
            &self.renderer.font_title_lg,
            RectF::new(x0, card.y + 16.0, w_full, 26.0),
            pal.text_primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            subtitle,
            &self.renderer.font_body,
            RectF::new(x0, card.y + 44.0, w_full, 20.0),
            pal.text_secondary,
            0,
            1,
            false,
        );
        self.renderer.draw_line(
            card.x + 20.0,
            card.y + 74.0,
            card.x + card.w - 20.0,
            card.y + 74.0,
            1.0,
            pal.outline_subtle,
        );
    }

    pub(crate) fn draw_card_footer(
        &self,
        card: RectF,
        hint: &str,
        next_label: &str,
        next_icon: Option<VectorIcon>,
    ) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let footer_line_y = card.y + card.h - 62.0;

        self.renderer.draw_line(
            card.x + 20.0,
            footer_line_y,
            card.x + card.w - 20.0,
            footer_line_y,
            1.0,
            pal.outline_subtle,
        );

        self.renderer.draw_text(
            hint,
            &self.renderer.font_caption,
            RectF::new(card.x + 28.0, card.y + card.h - 50.0, card.w - 236.0, 38.0),
            pal.text_muted,
            0,
            1,
            false,
        );

        let btn_next = Self::card_footer_next_btn(card);
        self.renderer.draw_button(
            btn_next,
            next_label,
            next_icon,
            ButtonVariant::Filled,
            btn_next.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );
    }

    pub fn draw(&mut self) {
        if !self.hwnd.0.is_null() {
            let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
            if self.renderer.ensure_target(self.hwnd, dpi).is_err() {
                return;
            }
        } else if self
            .renderer
            .ensure_surface(self.width_dip as u32, self.height_dip as u32, 96)
            .is_err()
        {
            return;
        }

        let w = self.width_dip;
        let h = self.height_dip;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let pal = self.palette;

        unsafe {
            self.renderer.rt().BeginDraw();
        }

        // 1. Window background & Left Navigation Rail
        self.renderer.fill_rect(RectF::new(0.0, 0.0, w, h), pal.bg);
        let nav_rect = self.nav_rail_rect();
        self.renderer.fill_rounded_rect(nav_rect, 14.0, pal.nav_bg);

        // 2. Custom Borderless Titlebar
        let hover_theme = D2DRenderer::header_theme_rect(w).contains(mx, my);
        let hover_min = D2DRenderer::header_min_rect(w).contains(mx, my);
        let hover_close = D2DRenderer::header_close_rect(w).contains(mx, my);
        self.renderer.draw_window_header(
            w,
            "Nyan Setup Studio",
            "",
            &pal,
            hover_theme,
            hover_min,
            hover_close,
        );

        // 3. Left Navigation Rail Items
        self.renderer.draw_text(
            "打包配置步骤",
            &self.renderer.font_caption,
            RectF::new(28.0, 62.0, 170.0, 20.0),
            pal.text_muted,
            0,
            1,
            false,
        );

        let tabs = [
            (StudioTab::Metadata, VectorIcon::Info, "1. 应用元数据"),
            (StudioTab::FilesAndPaths, VectorIcon::Folder, "2. 目录与路径"),
            (StudioTab::OptionsAndLicense, VectorIcon::Shield, "3. 选项与协议"),
            (StudioTab::Prerequisites, VectorIcon::Sparkles, "4. 环境检测"),
            (StudioTab::BuildAndTest, VectorIcon::Rocket, "5. 构建与测试"),
        ];

        for (i, (tab, icon, title)) in tabs.iter().enumerate() {
            let r = Self::nav_item_rect(i);
            let selected = self.active_tab == *tab;
            let hovered = r.contains(mx, my);

            if selected {
                self.renderer.fill_rounded_rect(r, 19.0, pal.primary);
            } else if hovered {
                self.renderer
                    .fill_rounded_rect(r, 19.0, rgba(0x6366F1, 0.12));
            }
            self.renderer.draw_ripples(&self.ripples, r);

            let fg = if selected {
                rgb(0xFFFFFF)
            } else {
                pal.text_primary
            };
            self.renderer
                .draw_icon(*icon, r.x + 22.0, r.y + r.h * 0.5, 15.0, fg);
            let t_rect = RectF::new(r.x + 42.0, r.y, r.w - 48.0, r.h);
            let font = if selected {
                &self.renderer.font_body_bold
            } else {
                &self.renderer.font_body
            };
            self.renderer.draw_text(title, font, t_rect, fg, 0, 1, false);
        }

        // Summary box inside left rail
        let stat_card = RectF::new(22.0, 316.0, 186.0, 238.0);
        self.renderer.fill_rounded_rect(stat_card, 12.0, pal.surface);
        self.renderer
            .draw_rounded_rect(stat_card, 12.0, 1.0, pal.outline_subtle);

        self.renderer.draw_text(
            "当前源目录统计",
            &self.renderer.font_caption,
            RectF::new(36.0, 326.0, 158.0, 18.0),
            pal.text_secondary,
            0,
            1,
            false,
        );
        self.renderer.draw_line(
            32.0,
            348.0,
            198.0,
            348.0,
            1.0,
            pal.outline_subtle,
        );

        self.renderer.draw_text(
            &format!("{} 个文件", self.scanned_files.len()),
            &self.renderer.font_title_lg,
            RectF::new(36.0, 356.0, 158.0, 28.0),
            pal.primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            &format!("未压缩体积: {}", format_bytes(self.scanned_total_bytes)),
            &self.renderer.font_body_bold,
            RectF::new(36.0, 386.0, 158.0, 20.0),
            pal.text_primary,
            0,
            1,
            false,
        );

        let info_rows = [
            "渲染引擎: Direct2D 硬件加速",
            "运行库: 静态链接 CRT",
            "压缩流: LZMA2 固实 + BCJ",
            "卸载器: 自动生成 unins000",
        ];
        for (idx, line) in info_rows.iter().enumerate() {
            let ry = 420.0 + (idx as f32) * 25.0;
            self.renderer.fill_circle(38.0, ry + 9.0, 3.0, pal.success);
            self.renderer.draw_text(
                line,
                &self.renderer.font_caption,
                RectF::new(48.0, ry, 154.0, 18.0),
                pal.text_secondary,
                0,
                1,
                false,
            );
        }

        let demo_btn = self.btn_generate_demo_rect();
        self.renderer.draw_button(
            demo_btn,
            "生成演示项目",
            Some(VectorIcon::Sparkles),
            ButtonVariant::Outlined,
            demo_btn.contains(mx, my),
            !self.is_building,
            &pal,
            &self.ripples,
        );

        // 4. Right Content Card
        let card = self.content_card_rect();
        self.renderer.draw_card(card, 14.0, &pal);

        match self.active_tab {
            StudioTab::Metadata => self.draw_tab_metadata(card),
            StudioTab::FilesAndPaths => self.draw_tab_files(card),
            StudioTab::OptionsAndLicense => self.draw_tab_options(card),
            StudioTab::Prerequisites => self.draw_tab_prerequisites(card),
            StudioTab::BuildAndTest => self.draw_tab_build(card),
        }

        // Subtle outer border around the borderless window
        self.renderer.draw_rounded_rect(
            RectF::new(0.5, 0.5, w - 1.0, h - 1.0),
            8.0,
            1.0,
            pal.outline,
        );

        let hr = unsafe { self.renderer.rt().EndDraw(None, None) };
        if hr.is_err() {
            self.renderer.discard_target();
        }
    }

    pub(crate) fn draw_tab_metadata(&self, card: RectF) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 28.0;
        let w_full = card.w - 56.0;
        let w_half = (w_full - 20.0) * 0.5;

        self.draw_card_header(
            card,
            "应用基本元数据配置",
            "设置安装向导展示的应用名称、版本号、发布者及注册表卸载标识。",
        );

        // Row 1 (y = card.y + 112.0)
        let r_name = RectF::new(x0, card.y + 112.0, w_half, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_name,
            "应用程序名称 (App Name)",
            &self.config.app_name,
            self.focused_field == FocusedField::AppName,
            r_name.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );

        let r_ver = RectF::new(x0 + w_half + 20.0, card.y + 112.0, w_half, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_ver,
            "版本号 (App Version)",
            &self.config.app_version,
            self.focused_field == FocusedField::AppVersion,
            r_ver.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );

        // Row 2 (y = card.y + 182.0)
        let r_pub = RectF::new(x0, card.y + 182.0, w_half, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_pub,
            "发布者 / 开发者名称 (Publisher)",
            &self.config.publisher,
            self.focused_field == FocusedField::Publisher,
            r_pub.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );

        let r_id = RectF::new(x0 + w_half + 20.0, card.y + 182.0, w_half, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_id,
            "应用唯一标识符 (App ID / 注册表卸载键名)",
            &self.config.app_id,
            self.focused_field == FocusedField::AppId,
            r_id.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );

        // Row 3 (y = card.y + 252.0): Main Exe (left) & Installer Icon (right)
        let r_exe = RectF::new(x0, card.y + 252.0, w_half - 100.0, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_exe,
            "主程序相对路径 (指向 .exe)",
            &self.config.main_exe,
            self.focused_field == FocusedField::MainExe,
            r_exe.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );

        let btn_pick_exe = RectF::new(x0 + w_half - 92.0, card.y + 252.0, 92.0, 38.0);
        self.renderer.draw_button(
            btn_pick_exe,
            "选择exe",
            Some(VectorIcon::Folder),
            ButtonVariant::Outlined,
            btn_pick_exe.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        let r_ico = RectF::new(x0 + w_half + 20.0, card.y + 252.0, w_half - 100.0, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_ico,
            "安装包图标 (可选 .ico，留空默认)",
            &self.installer_icon_input,
            self.focused_field == FocusedField::InstallerIcon,
            r_ico.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );

        let btn_pick_ico = RectF::new(x0 + w_full - 92.0, card.y + 252.0, 92.0, 38.0);
        self.renderer.draw_button(
            btn_pick_ico,
            "选择ico",
            Some(VectorIcon::Folder),
            ButtonVariant::Outlined,
            btn_pick_ico.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        // Live Brand Preview Card (y = card.y + 314.0, h = 176.0)
        let preview_box = RectF::new(x0, card.y + 314.0, w_full, 176.0);
        self.renderer
            .fill_rounded_rect(preview_box, 12.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(preview_box, 12.0, 1.0, pal.outline_subtle);

        self.renderer.draw_text(
            "安装向导品牌卡片与系统注册信息实时预览",
            &self.renderer.font_caption,
            RectF::new(preview_box.x + 18.0, preview_box.y + 12.0, 360.0, 18.0),
            pal.text_muted,
            0,
            1,
            false,
        );

        let icon_box = RectF::new(preview_box.x + 18.0, preview_box.y + 42.0, 52.0, 52.0);
        self.renderer
            .draw_app_icon_with_override(icon_box, Some(&self.installer_icon_input));

        self.renderer.draw_text(
            &format!("{} v{}", self.config.app_name, self.config.app_version),
            &self.renderer.font_title_md,
            RectF::new(preview_box.x + 84.0, preview_box.y + 40.0, 520.0, 24.0),
            pal.text_primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            &format!(
                "发布者: {}   |   启动入口: {}",
                self.config.publisher, self.config.main_exe
            ),
            &self.renderer.font_body,
            RectF::new(preview_box.x + 84.0, preview_box.y + 68.0, 520.0, 22.0),
            pal.text_secondary,
            0,
            1,
            false,
        );

        self.renderer.draw_line(
            preview_box.x + 18.0,
            preview_box.y + 108.0,
            preview_box.x + preview_box.w - 18.0,
            preview_box.y + 108.0,
            1.0,
            pal.outline_subtle,
        );

        self.renderer.draw_text(
            &format!(
                "快捷方式: {}.lnk   |   卸载项: Uninstall\\{}",
                self.config.app_name, self.config.app_id
            ),
            &self.renderer.font_mono,
            RectF::new(
                preview_box.x + 18.0,
                preview_box.y + 120.0,
                preview_box.w - 36.0,
                20.0,
            ),
            pal.text_secondary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            &format!("默认安装路径: {}", self.config.default_install_dir),
            &self.renderer.font_mono,
            RectF::new(
                preview_box.x + 18.0,
                preview_box.y + 144.0,
                preview_box.w - 36.0,
                20.0,
            ),
            pal.primary,
            0,
            1,
            false,
        );

        self.draw_card_footer(
            card,
            "提示：支持鼠标拖拽选中、Ctrl+A 全选、Ctrl+C/X/V 复制粘贴与方向键移动光标",
            "下一步：目录与路径",
            None,
        );
    }

    pub(crate) fn draw_tab_files(&self, card: RectF) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 28.0;
        let w_full = card.w - 56.0;

        self.draw_card_header(
            card,
            "打包源目录与输出路径配置",
            "选择需要打包发布的应用程序文件夹，并指定生成的单文件安装包保存位置。",
        );

        // 1. Source Directory (y = card.y + 112.0)
        let r_src = RectF::new(x0, card.y + 112.0, w_full - 138.0, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_src,
            "待打包的源文件夹路径 (Source Directory)",
            &self.source_dir,
            self.focused_field == FocusedField::SourceDir,
            r_src.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );
        let btn_src = RectF::new(x0 + w_full - 126.0, card.y + 112.0, 126.0, 38.0);
        self.renderer.draw_button(
            btn_src,
            "浏览目录",
            Some(VectorIcon::Folder),
            ButtonVariant::Outlined,
            btn_src.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        // 2. Default Target Install Directory (y = card.y + 180.0)
        let r_inst = RectF::new(x0, card.y + 180.0, w_full - 246.0, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_inst,
            "默认目标安装路径 (支持 %LOCALAPPDATA% 或 %PROGRAMFILES%)",
            &self.config.default_install_dir,
            self.focused_field == FocusedField::DefaultInstallDir,
            r_inst.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );
        let btn_localappdata = RectF::new(x0 + w_full - 234.0, card.y + 180.0, 112.0, 38.0);
        self.renderer.draw_button(
            btn_localappdata,
            "LocalAppData",
            None,
            ButtonVariant::Tonal,
            btn_localappdata.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );
        let btn_progfiles = RectF::new(x0 + w_full - 114.0, card.y + 180.0, 114.0, 38.0);
        self.renderer.draw_button(
            btn_progfiles,
            "Program Files",
            None,
            ButtonVariant::Tonal,
            btn_progfiles.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        // 3. Output Installer .exe path (y = card.y + 248.0)
        let r_out = RectF::new(x0, card.y + 248.0, w_full - 138.0, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_out,
            "生成的单文件安装包保存路径 (Output Setup .exe)",
            &self.output_exe,
            self.focused_field == FocusedField::OutputExe,
            r_out.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );
        let btn_out = RectF::new(x0 + w_full - 126.0, card.y + 248.0, 126.0, 38.0);
        self.renderer.draw_button(
            btn_out,
            "另存为",
            Some(VectorIcon::Document),
            ButtonVariant::Outlined,
            btn_out.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        // 4. Scanned Files Table Card (y = card.y + 302.0, h = 192.0)
        let list_box = RectF::new(x0, card.y + 302.0, w_full, 192.0);
        self.renderer
            .fill_rounded_rect(list_box, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(list_box, 10.0, 1.0, pal.outline_subtle);

        let header_bar = RectF::new(list_box.x, list_box.y, list_box.w, 30.0);
        self.renderer
            .fill_rounded_rect(header_bar, 10.0, rgba(0x6366F1, 0.08));
        self.renderer.draw_text(
            &format!(
                "源目录待打包文件列表 (共 {} 个文件，总计 {})",
                self.scanned_files.len(),
                format_bytes(self.scanned_total_bytes)
            ),
            &self.renderer.font_caption,
            RectF::new(list_box.x + 14.0, list_box.y + 5.0, list_box.w - 140.0, 20.0),
            pal.text_primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            "原始大小",
            &self.renderer.font_caption,
            RectF::new(
                list_box.x + list_box.w - 114.0,
                list_box.y + 5.0,
                98.0,
                20.0,
            ),
            pal.text_secondary,
            2,
            1,
            false,
        );

        if self.scanned_files.is_empty() {
            self.renderer.draw_text(
                "当前源目录下没有文件。请检查路径，或点击左下角“生成演示项目”。",
                &self.renderer.font_body,
                list_box,
                pal.warning,
                1,
                1,
                false,
            );
        } else {
            let max_visible = 6usize;
            let start = self
                .file_scroll_offset
                .min(self.scanned_files.len().saturating_sub(1));
            for (row, (rel, sz)) in self
                .scanned_files
                .iter()
                .skip(start)
                .take(max_visible)
                .enumerate()
            {
                let ry = list_box.y + 34.0 + (row as f32) * 25.0;
                if row % 2 == 1 {
                    self.renderer.fill_rounded_rect(
                        RectF::new(list_box.x + 6.0, ry, list_box.w - 12.0, 23.0),
                        4.0,
                        rgba(0x6366F1, 0.04),
                    );
                }
                self.renderer.draw_text(
                    rel,
                    &self.renderer.font_mono,
                    RectF::new(list_box.x + 14.0, ry, list_box.w - 130.0, 23.0),
                    pal.text_primary,
                    0,
                    1,
                    false,
                );
                self.renderer.draw_text(
                    &format_bytes(*sz),
                    &self.renderer.font_mono,
                    RectF::new(list_box.x + list_box.w - 114.0, ry, 98.0, 23.0),
                    pal.text_secondary,
                    2,
                    1,
                    false,
                );
            }
        }

        self.draw_card_footer(
            card,
            "提示：可在文件列表区域使用鼠标滚轮上下滚动查看全部文件",
            "下一步：选项与协议",
            None,
        );
    }

    pub(crate) fn draw_tab_options(&self, card: RectF) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 28.0;
        let w_full = card.w - 56.0;
        let left_w = 304.0;
        let right_x = x0 + left_w + 20.0;
        let right_w = w_full - left_w - 20.0;

        self.draw_card_header(
            card,
            "安装行为选项与软件许可协议",
            "配置快捷方式生成、控制面板卸载注册、数据压缩比及终端用户许可协议 (EULA)。",
        );

        // Left column: 4 Checkboxes + Process Conflict Policy + Compression Level
        self.renderer.draw_text(
            "默认快捷方式与安装行为",
            &self.renderer.font_body_bold,
            RectF::new(x0 + 4.0, card.y + 84.0, left_w, 20.0),
            pal.text_primary,
            0,
            1,
            false,
        );

        let c1 = RectF::new(x0, card.y + 108.0, left_w, 42.0);
        self.renderer.draw_checkbox(
            c1,
            self.config.create_desktop_shortcut,
            c1.contains(mx, my),
            "创建桌面快捷方式",
            Some("在用户桌面生成指向主程序的 .lnk 图标"),
            &pal,
        );

        let c2 = RectF::new(x0, card.y + 152.0, left_w, 42.0);
        self.renderer.draw_checkbox(
            c2,
            self.config.create_start_menu_shortcut,
            c2.contains(mx, my),
            "创建开始菜单快捷方式",
            Some("在 Windows 开始菜单程序组创建快捷方式"),
            &pal,
        );

        let c3 = RectF::new(x0, card.y + 196.0, left_w, 42.0);
        self.renderer.draw_checkbox(
            c3,
            self.config.register_uninstaller,
            c3.contains(mx, my),
            "生成卸载程序并注册系统卸载项",
            Some("释放 unins000.exe 并注册到“应用和功能”"),
            &pal,
        );

        let c4 = RectF::new(x0, card.y + 240.0, left_w, 42.0);
        self.renderer.draw_checkbox(
            c4,
            self.config.auto_launch_after_install,
            c4.contains(mx, my),
            "安装完成页默认勾选“立即运行”",
            Some("安装结束时允许用户一键启动主程序"),
            &pal,
        );

        // Process Conflict Policy Selector (y = card.y + 292..374)
        self.renderer.draw_text(
            "安装/更新时进程占用处理策略",
            &self.renderer.font_body_bold,
            RectF::new(x0 + 4.0, card.y + 292.0, left_w, 20.0),
            pal.text_primary,
            0,
            1,
            false,
        );

        let pol_w = (left_w - 8.0) * 0.5;
        let btn_pol_auto = RectF::new(x0, card.y + 316.0, pol_w, 34.0);
        let btn_pol_prompt = RectF::new(x0 + pol_w + 8.0, card.y + 316.0, pol_w, 34.0);
        self.renderer.draw_button(
            btn_pol_auto,
            "自动结束占用进程",
            None,
            if self.config.process_conflict_policy == ProcessConflictPolicy::AutoKill {
                ButtonVariant::Filled
            } else {
                ButtonVariant::Outlined
            },
            btn_pol_auto.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );
        self.renderer.draw_button(
            btn_pol_prompt,
            "弹出提示窗口",
            None,
            if self.config.process_conflict_policy == ProcessConflictPolicy::PromptUser {
                ButtonVariant::Filled
            } else {
                ButtonVariant::Outlined
            },
            btn_pol_prompt.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );
        let pol_hint = match self.config.process_conflict_policy {
            ProcessConflictPolicy::AutoKill => "检测到目标程序运行时，自动静默结束进程并继续安装",
            ProcessConflictPolicy::PromptUser => {
                "检测到占用时弹窗列出运行进程，提供“重试”与“结束进程并重试”"
            }
        };
        self.renderer.draw_text(
            pol_hint,
            &self.renderer.font_caption,
            RectF::new(x0 + 4.0, card.y + 354.0, left_w, 18.0),
            pal.text_secondary,
            0,
            1,
            false,
        );

        // Compression Level Selector (y = card.y + 382..444)
        self.renderer.draw_text(
            "LZMA2 固实压缩 + x86 BCJ 字典等级",
            &self.renderer.font_body_bold,
            RectF::new(x0 + 4.0, card.y + 382.0, left_w, 20.0),
            pal.text_primary,
            0,
            1,
            false,
        );

        let levels = [(1u32, "快速 (Lv.1)"), (6u32, "标准 (Lv.6)"), (9u32, "极限 (Lv.9)")];
        let seg_w = (left_w - 16.0) / 3.0;
        for (i, (lv, label)) in levels.iter().enumerate() {
            let sr = RectF::new(x0 + (i as f32) * (seg_w + 8.0), card.y + 408.0, seg_w, 36.0);
            let variant = if self.config.compression_level == *lv {
                ButtonVariant::Filled
            } else {
                ButtonVariant::Outlined
            };
            self.renderer.draw_button(
                sr,
                label,
                None,
                variant,
                sr.contains(mx, my),
                true,
                &pal,
                &self.ripples,
            );
        }

        // Right column: License Agreement Preview & Import
        self.renderer.draw_text(
            "软件许可协议内容 (支持鼠标滚轮滚动)",
            &self.renderer.font_body_bold,
            RectF::new(right_x, card.y + 88.0, right_w, 22.0),
            pal.text_primary,
            0,
            1,
            false,
        );

        let eula_box = RectF::new(right_x, card.y + 114.0, right_w, 332.0);
        self.renderer
            .fill_rounded_rect(eula_box, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(eula_box, 10.0, 1.0, pal.outline_subtle);

        let lines: Vec<&str> = self.config.license_text.lines().collect();
        let start_line = self.license_scroll_line.min(lines.len().saturating_sub(1));
        let visible_text = lines
            .into_iter()
            .skip(start_line)
            .take(13)
            .collect::<Vec<_>>()
            .join("\n");

        let inner_eula = eula_box.inset(12.0, 10.0);
        unsafe {
            self.renderer
                .rt()
                .PushAxisAlignedClip(&inner_eula.to_d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        self.renderer.draw_text(
            &visible_text,
            &self.renderer.font_caption,
            inner_eula,
            pal.text_secondary,
            0,
            0,
            true,
        );
        unsafe {
            self.renderer.rt().PopAxisAlignedClip();
        }

        let btn_load_eula = RectF::new(right_x, card.y + 456.0, (right_w - 10.0) * 0.5, 34.0);
        self.renderer.draw_button(
            btn_load_eula,
            "导入协议文件",
            Some(VectorIcon::Document),
            ButtonVariant::Outlined,
            btn_load_eula.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        let btn_paste_eula = RectF::new(
            right_x + (right_w - 10.0) * 0.5 + 10.0,
            card.y + 456.0,
            (right_w - 10.0) * 0.5,
            34.0,
        );
        self.renderer.draw_button(
            btn_paste_eula,
            "从剪贴板粘贴",
            None,
            ButtonVariant::Tonal,
            btn_paste_eula.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        self.draw_card_footer(
            card,
            "提示：用户安装软件时需勾选同意上述许可协议方可继续安装",
            "下一步：环境检测",
            None,
        );
    }

    pub(crate) fn draw_tab_build(&self, card: RectF) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 28.0;
        let w_full = card.w - 56.0;

        self.draw_card_header(
            card,
            "构建独立单文件安装程序 (Build & Verify)",
            "将当前配置与源目录全部文件压缩封装至静态 CRT Stub 中，输出独立 .exe 安装包。",
        );

        // Summary Banner Card (y = card.y + 90.0, h = 84.0)
        let sum_box = RectF::new(x0, card.y + 90.0, w_full, 84.0);
        self.renderer
            .fill_rounded_rect(sum_box, 12.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(sum_box, 12.0, 1.0, pal.outline_subtle);

        self.renderer.draw_text(
            &format!(
                "目标应用: {} v{} ({})   |   待打包文件: {} 个 ({})",
                self.config.app_name,
                self.config.app_version,
                self.config.publisher,
                self.scanned_files.len(),
                format_bytes(self.scanned_total_bytes)
            ),
            &self.renderer.font_body_bold,
            RectF::new(sum_box.x + 16.0, sum_box.y + 12.0, sum_box.w - 32.0, 22.0),
            pal.text_primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            &format!("输出安装包路径: {}", self.output_exe),
            &self.renderer.font_mono,
            RectF::new(sum_box.x + 16.0, sum_box.y + 36.0, sum_box.w - 32.0, 20.0),
            pal.primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            "静态 CRT 链接: target-feature=+crt-static (无需目标机器安装 VC++ 运行库，单文件直接运行)",
            &self.renderer.font_caption,
            RectF::new(sum_box.x + 16.0, sum_box.y + 58.0, sum_box.w - 32.0, 18.0),
            pal.success,
            0,
            1,
            false,
        );

        // Progress bar & Status text (y = card.y + 190..236)
        self.renderer.draw_text(
            &self.build_status_text,
            &self.renderer.font_body_bold,
            RectF::new(x0, card.y + 190.0, w_full - 70.0, 22.0),
            pal.text_primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            &format!("{:.0}%", self.build_progress * 100.0),
            &self.renderer.font_body_bold,
            RectF::new(x0 + w_full - 64.0, card.y + 190.0, 64.0, 22.0),
            pal.primary,
            2,
            1,
            false,
        );

        let prog_rect = RectF::new(x0, card.y + 220.0, w_full, 10.0);
        self.renderer
            .draw_progress_bar(prog_rect, self.build_progress, &pal);

        // Build Log Console Card (y = card.y + 244.0, h = 252.0)
        let log_box = RectF::new(x0, card.y + 244.0, w_full, 252.0);
        self.renderer
            .fill_rounded_rect(log_box, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(log_box, 10.0, 1.0, pal.outline_subtle);

        let wrapped_logs = self.renderer.wrap_log_lines(
            &self.build_logs,
            &self.renderer.font_mono,
            log_box.w - 28.0,
        );
        let max_logs = 10usize;
        let skip = wrapped_logs.len().saturating_sub(max_logs);
        let log_clip = log_box.inset(8.0, 6.0);
        unsafe {
            self.renderer
                .rt()
                .PushAxisAlignedClip(&log_clip.to_d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        for (i, (orig, line)) in wrapped_logs.iter().skip(skip).enumerate() {
            let ly = log_box.y + 12.0 + (i as f32) * 23.0;
            let col = if orig.starts_with("[错误]") {
                pal.error
            } else if orig.starts_with("[完成]") || orig.starts_with("[输出文件]") {
                pal.success
            } else {
                pal.text_secondary
            };
            self.renderer.draw_text(
                line,
                &self.renderer.font_mono,
                RectF::new(log_box.x + 14.0, ly, log_box.w - 28.0, 20.0),
                col,
                0,
                1,
                false,
            );
        }
        unsafe {
            self.renderer.rt().PopAxisAlignedClip();
        }

        // Footer bar with Build & Test actions
        let footer_line_y = card.y + card.h - 62.0;
        self.renderer.draw_line(
            card.x + 20.0,
            footer_line_y,
            card.x + card.w - 20.0,
            footer_line_y,
            1.0,
            pal.outline_subtle,
        );

        let btn_open_dir = RectF::new(x0, card.y + card.h - 50.0, 146.0, 38.0);
        self.renderer.draw_button(
            btn_open_dir,
            "打开输出目录",
            Some(VectorIcon::Folder),
            ButtonVariant::Outlined,
            btn_open_dir.contains(mx, my),
            self.last_built_exe.is_some() && !self.is_building,
            &pal,
            &self.ripples,
        );

        let btn_run = RectF::new(x0 + 158.0, card.y + card.h - 50.0, 186.0, 38.0);
        self.renderer.draw_button(
            btn_run,
            "运行测试安装包",
            Some(VectorIcon::Play),
            ButtonVariant::Tonal,
            btn_run.contains(mx, my),
            self.last_built_exe.is_some() && !self.is_building,
            &pal,
            &self.ripples,
        );

        let btn_build = RectF::new(card.x + card.w - 236.0, card.y + card.h - 50.0, 208.0, 38.0);
        let build_label = if self.is_building {
            "正在打包构建中..."
        } else {
            "开始构建安装包 (.exe)"
        };
        self.renderer.draw_button(
            btn_build,
            build_label,
            Some(VectorIcon::Rocket),
            ButtonVariant::Filled,
            btn_build.contains(mx, my),
            !self.is_building,
            &pal,
            &self.ripples,
        );
    }
}
