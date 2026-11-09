use super::{InstallerWindowState, WizardMode, WizardStep};
use crate::d2d::{rgb, rgba, ButtonVariant, D2DRenderer, RectF, VectorIcon};
use crate::package::{format_bytes, PackageManifest, UninstallReceipt};
use windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE;
use windows::Win32::UI::HiDpi::GetDpiForWindow;

impl InstallerWindowState {
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

        self.renderer.fill_rect(RectF::new(0.0, 0.0, w, h), pal.bg);

        let hover_theme = D2DRenderer::header_theme_rect(w).contains(mx, my);
        let hover_min = D2DRenderer::header_min_rect(w).contains(mx, my);
        let hover_close = D2DRenderer::header_close_rect(w).contains(mx, my);

        match &self.mode {
            WizardMode::Installer { manifest, .. } => {
                let is_update = self.existing_install.is_some();
                let title = if is_update {
                    format!(
                        "{} v{} 更新向导",
                        manifest.config.app_name, manifest.config.app_version
                    )
                } else {
                    format!(
                        "{} v{} 安装向导",
                        manifest.config.app_name, manifest.config.app_version
                    )
                };
                let badge = if is_update {
                    "覆盖更新模式"
                } else {
                    ""
                };
                self.renderer.draw_window_header(
                    w,
                    &title,
                    badge,
                    &pal,
                    hover_theme,
                    hover_min,
                    hover_close,
                );
                self.draw_installer_stepper();
                let card = self.installer_card_rect();
                self.renderer.draw_card(card, 14.0, &pal);

                let manifest_clone = manifest.clone();
                match self.step {
                    WizardStep::WelcomeAndLicense => self.draw_step_welcome(card, &manifest_clone),
                    WizardStep::CustomOptions => self.draw_step_options(card, &manifest_clone),
                    WizardStep::Installing => self.draw_step_installing(card),
                    WizardStep::Completed => self.draw_step_completed(card, &manifest_clone),
                }

                if self.show_process_conflict_modal {
                    self.draw_process_conflict_modal();
                }
            }
            WizardMode::Uninstaller { receipt, .. } => {
                let title = format!("{} 卸载向导", receipt.app_name);
                self.renderer.draw_window_header(
                    w,
                    &title,
                    "Uninstaller",
                    &pal,
                    hover_theme,
                    hover_min,
                    hover_close,
                );
                let card = RectF::new(18.0, 52.0, w - 36.0, h - 68.0);
                self.renderer.draw_card(card, 14.0, &pal);
                let receipt_clone = receipt.clone();
                self.draw_uninstaller_view(card, &receipt_clone);
            }
        }

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

    /// Draw a clean Pill Stepper where connector lines only span gaps between pills.
    pub(crate) fn draw_installer_stepper(&self) {
        let pal = self.palette;
        let steps = if self.existing_install.is_some() {
            ["许可协议", "更新选项", "正在更新", "更新完成"]
        } else {
            ["许可协议", "安装选项", "正在安装", "安装完成"]
        };
        let cur = self.step as usize;

        let pill_w = 148.0;
        let pill_h = 32.0;
        let gap = 44.0;
        let total_w = pill_w * 4.0 + gap * 3.0;
        let start_x = (self.width_dip - total_w) * 0.5;
        let pill_y = 54.0;
        let cy = pill_y + pill_h * 0.5;

        for (i, label) in steps.iter().enumerate() {
            let px = start_x + (i as f32) * (pill_w + gap);
            let pill_rect = RectF::new(px, pill_y, pill_w, pill_h);

            if i + 1 < steps.len() {
                let line_x1 = px + pill_w + 6.0;
                let line_x2 = px + pill_w + gap - 6.0;
                let line_col = if i < cur {
                    pal.success
                } else {
                    pal.outline
                };
                self.renderer
                    .draw_line(line_x1, cy, line_x2, cy, 2.0, line_col);
            }

            if i == cur {
                self.renderer
                    .fill_rounded_rect(pill_rect, 16.0, pal.primary_container);
                self.renderer
                    .draw_rounded_rect(pill_rect, 16.0, 1.2, pal.primary);
            } else if i < cur {
                self.renderer.fill_rounded_rect(pill_rect, 16.0, pal.surface);
                self.renderer
                    .draw_rounded_rect(pill_rect, 16.0, 1.0, pal.success);
            } else {
                self.renderer.fill_rounded_rect(pill_rect, 16.0, pal.surface);
                self.renderer
                    .draw_rounded_rect(pill_rect, 16.0, 1.0, pal.outline_subtle);
            }

            let circle_cx = px + 18.0;
            if i < cur {
                self.renderer.fill_circle(circle_cx, cy, 10.0, pal.success);
                self.renderer
                    .draw_icon(VectorIcon::Check, circle_cx, cy, 11.0, rgb(0xFFFFFF));
            } else if i == cur {
                self.renderer.fill_circle(circle_cx, cy, 10.0, pal.primary);
                self.renderer.draw_text(
                    &format!("{}", i + 1),
                    &self.renderer.font_caption,
                    RectF::new(circle_cx - 10.0, cy - 10.0, 20.0, 20.0),
                    rgb(0xFFFFFF),
                    1,
                    1,
                    false,
                );
            } else {
                self.renderer
                    .fill_circle(circle_cx, cy, 10.0, pal.surface_variant);
                self.renderer
                    .draw_circle(circle_cx, cy, 10.0, 1.2, pal.outline);
                self.renderer.draw_text(
                    &format!("{}", i + 1),
                    &self.renderer.font_caption,
                    RectF::new(circle_cx - 10.0, cy - 10.0, 20.0, 20.0),
                    pal.text_muted,
                    1,
                    1,
                    false,
                );
            }

            let text_col = if i == cur {
                pal.on_primary_container
            } else if i < cur {
                pal.text_primary
            } else {
                pal.text_muted
            };
            self.renderer.draw_text(
                label,
                &self.renderer.font_body_bold,
                RectF::new(px + 34.0, pill_y, pill_w - 40.0, pill_h),
                text_col,
                0,
                1,
                false,
            );
        }
    }

    pub(crate) fn draw_step_welcome(&self, card: RectF, manifest: &PackageManifest) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 26.0;
        let w_full = card.w - 52.0;

        let hero = RectF::new(x0, card.y + 18.0, w_full, 72.0);
        self.renderer
            .fill_rounded_rect(hero, 12.0, pal.primary_container);

        let logo_box = RectF::new(hero.x + 16.0, hero.y + 10.0, 52.0, 52.0);
        self.renderer.draw_nyan_app_icon(logo_box);

        let hero_title = if let Some(ref ex) = self.existing_install {
            format!(
                "检测到已安装 v{} — 升级/更新 {}",
                ex.installed_version, manifest.config.app_name
            )
        } else {
            format!("欢迎安装 {}", manifest.config.app_name)
        };
        let hero_sub = if self.existing_install.is_some() {
            format!(
                "目标版本: v{}   |   原路径: {}   |   共 {} 个文件 ({})",
                manifest.config.app_version,
                self.install_dir_input,
                manifest.files.len(),
                format_bytes(manifest.total_uncompressed_bytes)
            )
        } else {
            format!(
                "版本: v{}   |   发布者: {}   |   包含 {} 个文件 ({})",
                manifest.config.app_version,
                manifest.config.publisher,
                manifest.files.len(),
                format_bytes(manifest.total_uncompressed_bytes)
            )
        };

        self.renderer.draw_text(
            &hero_title,
            &self.renderer.font_title_lg,
            RectF::new(hero.x + 82.0, hero.y + 10.0, hero.w - 96.0, 26.0),
            pal.on_primary_container,
            0,
            1,
            false,
        );
        self.renderer.draw_text_ellipsis(
            &hero_sub,
            &self.renderer.font_body,
            RectF::new(hero.x + 82.0, hero.y + 40.0, hero.w - 96.0, 22.0),
            pal.on_primary_container,
            0,
            1,
        );

        self.renderer.draw_text(
            "软件许可协议 (请使用鼠标滚轮阅读全文):",
            &self.renderer.font_caption,
            RectF::new(x0, card.y + 96.0, w_full, 18.0),
            pal.text_secondary,
            0,
            1,
            false,
        );

        let eula_box = RectF::new(x0, card.y + 118.0, w_full, 232.0);
        self.renderer
            .fill_rounded_rect(eula_box, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(eula_box, 10.0, 1.0, pal.outline_subtle);

        let lines: Vec<&str> = manifest.config.license_text.lines().collect();
        let start = self.license_scroll_line.min(lines.len().saturating_sub(1));
        let visible = lines
            .into_iter()
            .skip(start)
            .take(12)
            .collect::<Vec<_>>()
            .join("\n");

        let inner = eula_box.inset(14.0, 10.0);
        unsafe {
            self.renderer
                .rt()
                .PushAxisAlignedClip(&inner.to_d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        self.renderer.draw_text(
            &visible,
            &self.renderer.font_caption,
            inner,
            pal.text_secondary,
            0,
            0,
            true,
        );
        unsafe {
            self.renderer.rt().PopAxisAlignedClip();
        }

        let footer_line_y = card.y + card.h - 62.0;
        self.renderer.draw_line(
            card.x + 20.0,
            footer_line_y,
            card.x + card.w - 20.0,
            footer_line_y,
            1.0,
            pal.outline_subtle,
        );

        let btn_y = card.y + card.h - 50.0;
        let chk_rect = RectF::new(x0, btn_y, 280.0, 38.0);
        self.renderer.draw_checkbox(
            chk_rect,
            self.license_accepted,
            chk_rect.contains(mx, my),
            "我已阅读并同意《软件许可协议》",
            None,
            &pal,
        );

        let btn_custom = RectF::new(card.x + card.w - 332.0, btn_y, 136.0, 38.0);
        self.renderer.draw_button(
            btn_custom,
            "自定义选项",
            Some(VectorIcon::Folder),
            ButtonVariant::Tonal,
            btn_custom.contains(mx, my),
            self.license_accepted,
            &pal,
            &self.ripples,
        );

        let quick_label = if self.existing_install.is_some() {
            "一键覆盖更新"
        } else {
            "一键快速安装"
        };
        let btn_quick = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);
        self.renderer.draw_button(
            btn_quick,
            quick_label,
            Some(VectorIcon::Rocket),
            ButtonVariant::Filled,
            btn_quick.contains(mx, my),
            self.license_accepted && self.has_enough_disk_space(),
            &pal,
            &self.ripples,
        );
    }

    pub(crate) fn draw_step_options(&self, card: RectF, manifest: &PackageManifest) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 26.0;
        let w_full = card.w - 52.0;

        let header_str = if let Some(ref ex) = self.existing_install {
            format!(
                "覆盖更新选项 (检测到已安装 v{} → 升级/更新至 v{})",
                ex.installed_version, manifest.config.app_version
            )
        } else {
            "自定义安装目录与系统快捷方式选项".to_string()
        };
        self.renderer.draw_text(
            &header_str,
            &self.renderer.font_title_md,
            RectF::new(x0, card.y + 16.0, w_full, 24.0),
            pal.text_primary,
            0,
            1,
            false,
        );

        let r_path = RectF::new(x0, card.y + 66.0, w_full - 134.0, 38.0);
        self.renderer.draw_text_field_with_sel(
            r_path,
            "目标安装文件夹路径 (Installation Directory)",
            &self.install_dir_input,
            self.path_focused,
            r_path.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );

        let btn_browse = RectF::new(x0 + w_full - 122.0, card.y + 66.0, 122.0, 38.0);
        self.renderer.draw_button(
            btn_browse,
            "浏览目录",
            Some(VectorIcon::Folder),
            ButtonVariant::Outlined,
            btn_browse.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        let disk_card = RectF::new(x0, card.y + 114.0, w_full, 60.0);
        self.renderer
            .fill_rounded_rect(disk_card, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(disk_card, 10.0, 1.0, pal.outline_subtle);
        self.renderer.draw_icon(
            VectorIcon::HardDrive,
            disk_card.x + 26.0,
            disk_card.y + 30.0,
            20.0,
            pal.primary,
        );

        let req_bytes = manifest.total_uncompressed_bytes;
        if let Some((free_b, total_b)) = self.disk_space {
            let enough = self.has_enough_disk_space();
            let status_str = if enough {
                "磁盘空间充足"
            } else {
                "磁盘空间不足！"
            };
            let status_col = if enough { pal.success } else { pal.error };

            self.renderer.draw_text(
                &format!(
                    "所需空间: {}   |   目标分区可用空间: {} (总容量 {})",
                    format_bytes(req_bytes),
                    format_bytes(free_b),
                    format_bytes(total_b)
                ),
                &self.renderer.font_body_bold,
                RectF::new(
                    disk_card.x + 50.0,
                    disk_card.y + 8.0,
                    disk_card.w - 170.0,
                    20.0,
                ),
                pal.text_primary,
                0,
                1,
                false,
            );
            self.renderer.draw_text(
                status_str,
                &self.renderer.font_body_bold,
                RectF::new(
                    disk_card.x + disk_card.w - 124.0,
                    disk_card.y + 8.0,
                    108.0,
                    20.0,
                ),
                status_col,
                2,
                1,
                false,
            );

            let used_ratio = if total_b > 0 {
                1.0 - (free_b as f32 / total_b as f32)
            } else {
                0.2
            };
            let bar_rect =
                RectF::new(disk_card.x + 50.0, disk_card.y + 36.0, disk_card.w - 66.0, 8.0);
            self.renderer.draw_progress_bar(bar_rect, used_ratio, &pal);
        } else {
            self.renderer.draw_text(
                &format!(
                    "所需安装空间: {} (无法检测目标盘符可用容量)",
                    format_bytes(req_bytes)
                ),
                &self.renderer.font_body,
                RectF::new(disk_card.x + 50.0, disk_card.y + 18.0, disk_card.w - 66.0, 24.0),
                pal.warning,
                0,
                1,
                false,
            );
        }

        let c1 = RectF::new(x0, card.y + 190.0, w_full, 40.0);
        self.renderer.draw_checkbox(
            c1,
            self.opt_desktop_shortcut,
            c1.contains(mx, my),
            "在桌面创建快捷方式图标 (Desktop Shortcut)",
            None,
            &pal,
        );

        let c2 = RectF::new(x0, card.y + 234.0, w_full, 40.0);
        self.renderer.draw_checkbox(
            c2,
            self.opt_start_menu_shortcut,
            c2.contains(mx, my),
            "在开始菜单程序文件夹创建快捷方式 (Start Menu Shortcut)",
            None,
            &pal,
        );

        let c3 = RectF::new(x0, card.y + 278.0, w_full, 40.0);
        self.renderer.draw_checkbox(
            c3,
            self.opt_register_uninstaller,
            c3.contains(mx, my),
            "生成卸载程序并注册到 Windows“应用和功能”列表",
            None,
            &pal,
        );

        let footer_line_y = card.y + card.h - 62.0;
        self.renderer.draw_line(
            card.x + 20.0,
            footer_line_y,
            card.x + card.w - 20.0,
            footer_line_y,
            1.0,
            pal.outline_subtle,
        );

        let btn_y = card.y + card.h - 50.0;
        let btn_back = RectF::new(x0, btn_y, 120.0, 38.0);
        self.renderer.draw_button(
            btn_back,
            "返回上一步",
            None,
            ButtonVariant::Outlined,
            btn_back.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        let start_label = if self.existing_install.is_some() {
            "开始更新"
        } else {
            "开始安装"
        };
        let btn_install = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);
        self.renderer.draw_button(
            btn_install,
            start_label,
            Some(VectorIcon::Rocket),
            ButtonVariant::Filled,
            btn_install.contains(mx, my),
            self.has_enough_disk_space() && !self.install_dir_input.trim().is_empty(),
            &pal,
            &self.ripples,
        );
    }

    pub(crate) fn draw_process_conflict_modal(&self) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let w = self.width_dip;
        let h = self.height_dip;

        self.renderer
            .fill_rect(RectF::new(0.0, 46.0, w, h - 46.0), rgba(0x000000, 0.48));

        let modal = self.process_conflict_modal_rect();
        for i in (1..=4).rev() {
            let sp = i as f32 * 3.0;
            let sr = RectF::new(
                modal.x - sp * 0.5,
                modal.y + sp * 0.8,
                modal.w + sp,
                modal.h + sp,
            );
            self.renderer
                .fill_rounded_rect(sr, 16.0, rgba(0x000000, 0.12 / (i as f32)));
        }
        self.renderer
            .fill_rounded_rect(modal, 14.0, pal.surface_elevated);
        self.renderer
            .draw_rounded_rect(modal, 14.0, 1.5, pal.warning);

        let icon_cx = modal.x + 34.0;
        let icon_cy = modal.y + 34.0;
        self.renderer
            .fill_circle(icon_cx, icon_cy, 16.0, rgba(0xF59E0B, 0.16));
        self.renderer
            .draw_icon(VectorIcon::Info, icon_cx, icon_cy, 18.0, pal.warning);

        self.renderer.draw_text(
            "检测到目标程序正在运行 (文件被占用)",
            &self.renderer.font_title_md,
            RectF::new(modal.x + 60.0, modal.y + 16.0, modal.w - 108.0, 22.0),
            pal.text_primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            "以下进程正在占用目标安装目录，请先手动关闭它们后点击“重试”，或选择“结束进程并重试”：",
            &self.renderer.font_caption,
            RectF::new(modal.x + 60.0, modal.y + 40.0, modal.w - 76.0, 20.0),
            pal.text_secondary,
            0,
            1,
            false,
        );

        let btn_close_x = RectF::new(modal.x + modal.w - 42.0, modal.y + 14.0, 28.0, 28.0);
        if btn_close_x.contains(mx, my) {
            self.renderer
                .fill_rounded_rect(btn_close_x, 6.0, rgba(0xEF4444, 0.16));
        }
        self.renderer.draw_icon(
            VectorIcon::Close,
            btn_close_x.x + 14.0,
            btn_close_x.y + 14.0,
            14.0,
            pal.text_secondary,
        );

        let list_box = RectF::new(modal.x + 22.0, modal.y + 72.0, modal.w - 44.0, 186.0);
        self.renderer
            .fill_rounded_rect(list_box, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(list_box, 10.0, 1.0, pal.outline_subtle);

        let header_bar = RectF::new(list_box.x, list_box.y, list_box.w, 28.0);
        self.renderer
            .fill_rounded_rect(header_bar, 10.0, rgba(0xF59E0B, 0.10));
        self.renderer.draw_text(
            &format!("正在运行的占用进程 (共 {} 个实例)", self.running_processes.len()),
            &self.renderer.font_caption,
            RectF::new(list_box.x + 14.0, list_box.y + 4.0, list_box.w - 130.0, 20.0),
            pal.text_primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text(
            "进程 PID",
            &self.renderer.font_caption,
            RectF::new(
                list_box.x + list_box.w - 104.0,
                list_box.y + 4.0,
                90.0,
                20.0,
            ),
            pal.text_secondary,
            2,
            1,
            false,
        );

        let max_rows = 5usize;
        for (idx, (pid, exe_name)) in self.running_processes.iter().take(max_rows).enumerate() {
            let ry = list_box.y + 34.0 + (idx as f32) * 28.0;
            if idx % 2 == 1 {
                self.renderer.fill_rounded_rect(
                    RectF::new(list_box.x + 6.0, ry, list_box.w - 12.0, 25.0),
                    4.0,
                    rgba(0x6366F1, 0.04),
                );
            }
            self.renderer
                .fill_circle(list_box.x + 18.0, ry + 12.5, 3.5, pal.warning);
            self.renderer.draw_text(
                exe_name,
                &self.renderer.font_mono,
                RectF::new(list_box.x + 30.0, ry, list_box.w - 140.0, 25.0),
                pal.text_primary,
                0,
                1,
                false,
            );
            self.renderer.draw_text(
                &format!("PID: {}", pid),
                &self.renderer.font_mono,
                RectF::new(list_box.x + list_box.w - 104.0, ry, 90.0, 25.0),
                pal.text_secondary,
                2,
                1,
                false,
            );
        }
        if self.running_processes.len() > max_rows {
            let extra = self.running_processes.len() - max_rows;
            self.renderer.draw_text(
                &format!("... 以及另外 {} 个同名运行实例", extra),
                &self.renderer.font_caption,
                RectF::new(
                    list_box.x + 18.0,
                    list_box.y + list_box.h - 20.0,
                    list_box.w - 36.0,
                    18.0,
                ),
                pal.text_muted,
                0,
                1,
                false,
            );
        }

        let btn_y = modal.y + modal.h - 54.0;
        let btn_cancel = RectF::new(modal.x + 22.0, btn_y, 96.0, 38.0);
        self.renderer.draw_button(
            btn_cancel,
            "取消",
            None,
            ButtonVariant::Outlined,
            btn_cancel.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        let btn_retry = RectF::new(modal.x + modal.w - 302.0, btn_y, 112.0, 38.0);
        self.renderer.draw_button(
            btn_retry,
            "重试",
            None,
            ButtonVariant::Tonal,
            btn_retry.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        let btn_kill_retry = RectF::new(modal.x + modal.w - 180.0, btn_y, 158.0, 38.0);
        self.renderer.draw_button(
            btn_kill_retry,
            "结束进程并重试",
            Some(VectorIcon::Rocket),
            ButtonVariant::Filled,
            btn_kill_retry.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );
    }

    pub(crate) fn draw_step_installing(&self, card: RectF) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 26.0;
        let w_full = card.w - 52.0;

        self.renderer.draw_text(
            &format!("{:.0}%", self.progress * 100.0),
            &self.renderer.font_title_lg,
            RectF::new(x0, card.y + 22.0, 120.0, 30.0),
            if self.install_error.is_some() {
                pal.error
            } else {
                pal.primary
            },
            0,
            1,
            false,
        );

        let status_color = if self.install_error.is_some() {
            pal.error
        } else {
            pal.text_primary
        };
        self.renderer.draw_text_ellipsis(
            &self.status_text,
            &self.renderer.font_body_bold,
            RectF::new(x0, card.y + 56.0, w_full, 22.0),
            status_color,
            0,
            1,
        );

        let bar = RectF::new(x0, card.y + 86.0, w_full, 10.0);
        self.renderer.draw_progress_bar(bar, self.progress, &pal);

        let log_box = RectF::new(x0, card.y + 112.0, w_full, 232.0);
        self.draw_scrollable_log_box(log_box, false);

        let footer_line_y = card.y + card.h - 62.0;
        self.renderer.draw_line(
            card.x + 20.0,
            footer_line_y,
            card.x + card.w - 20.0,
            footer_line_y,
            1.0,
            pal.outline_subtle,
        );

        let btn_y = card.y + card.h - 50.0;
        if self.install_error.is_some() && !self.is_working {
            let btn_back_opt = RectF::new(x0, btn_y, 124.0, 38.0);
            self.renderer.draw_button(
                btn_back_opt,
                "返回选项",
                None,
                ButtonVariant::Outlined,
                btn_back_opt.contains(mx, my),
                true,
                &pal,
                &self.ripples,
            );

            let btn_kill_retry = RectF::new(card.x + card.w - 368.0, btn_y, 192.0, 38.0);
            self.renderer.draw_button(
                btn_kill_retry,
                "结束占用进程并重试",
                Some(VectorIcon::Rocket),
                ButtonVariant::Tonal,
                btn_kill_retry.contains(mx, my),
                true,
                &pal,
                &self.ripples,
            );

            let btn_retry = RectF::new(card.x + card.w - 164.0, btn_y, 138.0, 38.0);
            self.renderer.draw_button(
                btn_retry,
                "直接重试",
                Some(VectorIcon::Rocket),
                ButtonVariant::Filled,
                btn_retry.contains(mx, my),
                true,
                &pal,
                &self.ripples,
            );
        } else {
            self.renderer.draw_text(
                "正在流式解压并执行 CRC32 完整性校验，请稍候...",
                &self.renderer.font_caption,
                RectF::new(x0, btn_y, w_full, 38.0),
                pal.text_muted,
                0,
                1,
                false,
            );
        }
    }

    pub(crate) fn draw_step_completed(&self, card: RectF, manifest: &PackageManifest) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 26.0;
        let w_full = card.w - 52.0;
        let cx = card.x + card.w * 0.5;

        self.renderer
            .fill_circle(cx, card.y + 72.0, 34.0, rgba(0x10B981, 0.16));
        self.renderer
            .fill_circle(cx, card.y + 72.0, 24.0, pal.success);
        self.renderer
            .draw_icon(VectorIcon::Check, cx, card.y + 72.0, 22.0, rgb(0xFFFFFF));

        let done_title = if self.existing_install.is_some() {
            format!(
                "{} 已成功更新至 v{}！",
                manifest.config.app_name, manifest.config.app_version
            )
        } else {
            format!("{} 已成功安装！", manifest.config.app_name)
        };
        self.renderer.draw_text(
            &done_title,
            &self.renderer.font_title_lg,
            RectF::new(x0, card.y + 122.0, w_full, 28.0),
            pal.text_primary,
            1,
            1,
            false,
        );

        self.renderer.draw_text_ellipsis(
            &format!("安装位置: {}", self.install_dir_input),
            &self.renderer.font_mono,
            RectF::new(x0, card.y + 156.0, w_full, 22.0),
            pal.text_secondary,
            1,
            1,
        );

        let chk_launch = RectF::new(cx - 170.0, card.y + 206.0, 340.0, 42.0);
        self.renderer.draw_checkbox(
            chk_launch,
            self.opt_launch_now,
            chk_launch.contains(mx, my),
            &format!(
                "立即运行 {} ({})",
                manifest.config.app_name, manifest.config.main_exe
            ),
            None,
            &pal,
        );

        let footer_line_y = card.y + card.h - 62.0;
        self.renderer.draw_line(
            card.x + 20.0,
            footer_line_y,
            card.x + card.w - 20.0,
            footer_line_y,
            1.0,
            pal.outline_subtle,
        );

        let btn_y = card.y + card.h - 50.0;
        let btn_open_folder = RectF::new(card.x + card.w - 332.0, btn_y, 136.0, 38.0);
        self.renderer.draw_button(
            btn_open_folder,
            "打开安装目录",
            Some(VectorIcon::Folder),
            ButtonVariant::Outlined,
            btn_open_folder.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );

        let btn_finish = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);
        self.renderer.draw_button(
            btn_finish,
            "完成 (Finish)",
            Some(VectorIcon::Check),
            ButtonVariant::Filled,
            btn_finish.contains(mx, my),
            true,
            &pal,
            &self.ripples,
        );
    }

    pub(crate) fn draw_uninstaller_view(&self, card: RectF, receipt: &UninstallReceipt) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 26.0;
        let w_full = card.w - 52.0;

        let banner = RectF::new(x0, card.y + 22.0, w_full, 74.0);
        self.renderer
            .fill_rounded_rect(banner, 12.0, rgba(0xEF4444, 0.12));
        self.renderer.draw_icon(
            VectorIcon::Trash,
            banner.x + 36.0,
            banner.y + 37.0,
            24.0,
            pal.error,
        );

        self.renderer.draw_text(
            &format!("卸载 {} v{}", receipt.app_name, receipt.app_version),
            &self.renderer.font_title_lg,
            RectF::new(banner.x + 72.0, banner.y + 12.0, banner.w - 88.0, 26.0),
            pal.text_primary,
            0,
            1,
            false,
        );
        self.renderer.draw_text_ellipsis(
            &format!(
                "目标目录: {} (共 {} 个文件)",
                receipt.install_dir,
                receipt.installed_files.len()
            ),
            &self.renderer.font_body,
            RectF::new(banner.x + 72.0, banner.y + 42.0, banner.w - 88.0, 22.0),
            pal.text_secondary,
            0,
            1,
        );

        let log_box = RectF::new(x0, card.y + 114.0, w_full, 270.0);

        let footer_line_y = card.y + card.h - 62.0;
        self.renderer.draw_line(
            card.x + 20.0,
            footer_line_y,
            card.x + card.w - 20.0,
            footer_line_y,
            1.0,
            pal.outline_subtle,
        );
        let btn_y = card.y + card.h - 50.0;

        if !self.uninstall_done {
            self.renderer
                .fill_rounded_rect(log_box, 10.0, pal.surface_variant);
            self.renderer
                .draw_rounded_rect(log_box, 10.0, 1.0, pal.outline_subtle);
            self.renderer.draw_text(
                "点击下方“确认卸载并清理”将从您的电脑中移除以下内容：\n\n\
                • 自动结束正在运行的目标程序进程\n\
                • 安装目录中的所有程序文件与组件\n\
                • 桌面快捷方式与开始菜单程序组快捷方式\n\
                • Windows 注册表中的“应用和功能”卸载项",
                &self.renderer.font_body,
                log_box.inset(20.0, 20.0),
                pal.text_primary,
                0,
                0,
                true,
            );

            let btn_cancel = RectF::new(card.x + card.w - 316.0, btn_y, 120.0, 38.0);
            self.renderer.draw_button(
                btn_cancel,
                "取消",
                None,
                ButtonVariant::Outlined,
                btn_cancel.contains(mx, my),
                true,
                &pal,
                &self.ripples,
            );

            let btn_unins = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);
            self.renderer.draw_button(
                btn_unins,
                "确认卸载并清理",
                Some(VectorIcon::Trash),
                ButtonVariant::Danger,
                btn_unins.contains(mx, my),
                true,
                &pal,
                &self.ripples,
            );
        } else {
            self.draw_scrollable_log_box(log_box, true);

            let btn_close = RectF::new(card.x + card.w - 184.0, btn_y, 158.0, 38.0);
            self.renderer.draw_button(
                btn_close,
                "完成并退出",
                Some(VectorIcon::Check),
                ButtonVariant::Filled,
                btn_close.contains(mx, my),
                true,
                &pal,
                &self.ripples,
            );
        }
    }
}
