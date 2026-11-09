use super::{FocusedField, PrereqDropdown, StudioTab, StudioWindowState};
use crate::d2d::{rgb, rgba, ButtonVariant, RectF, VectorIcon};
use crate::package::{
    CheckOperator, PrereqAction, PrereqCondition, PrerequisiteRule, RegArchView, RegHiveRoot,
};
use crate::win32_ops::{evaluate_prereq_condition, pick_open_file_dialog};

impl StudioWindowState {
    pub(crate) fn draw_tab_prerequisites(&self, card: RectF) {
        let pal = self.palette;
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let x0 = card.x + 28.0;
        let w_full = card.w - 56.0;

        self.draw_card_header(
            card,
            "前置运行环境检测与自动部署 (Prerequisites)",
            "配置安装时需校验的注册表键值或系统文件/PE版本，未满足时自动运行随包组件或自定义逻辑。",
        );

        // Top Preset Bar (y = card.y + 84.0, h = 34.0)
        let presets = [
            "+ VC++ 2015-2022 (x64)",
            "+ VC++ 2015-2022 (x86)",
            "+ 注册表检测规则",
            "+ 文件/PE版本检测",
        ];
        let preset_w = (w_full - 30.0) / 4.0;
        for (i, label) in presets.iter().enumerate() {
            let pr = RectF::new(
                x0 + (i as f32) * (preset_w + 10.0),
                card.y + 84.0,
                preset_w,
                34.0,
            );
            self.renderer.draw_button(
                pr,
                label,
                None,
                ButtonVariant::Tonal,
                pr.contains(mx, my),
                true,
                &pal,
                &self.ripples,
            );
        }

        // Left Rule List Box (x = x0, y = card.y + 128.0, w = 216.0, h = 368.0)
        let list_w = 216.0;
        let list_box = RectF::new(x0, card.y + 128.0, list_w, 368.0);
        self.renderer
            .fill_rounded_rect(list_box, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(list_box, 10.0, 1.0, pal.outline_subtle);

        self.renderer.draw_text(
            &format!("检测规则列表 ({})", self.config.prerequisites.len()),
            &self.renderer.font_caption,
            RectF::new(list_box.x + 12.0, list_box.y + 8.0, list_w - 24.0, 18.0),
            pal.text_secondary,
            0,
            1,
            false,
        );

        let max_rules_vis = 6usize;
        for (idx, rule) in self
            .config
            .prerequisites
            .iter()
            .take(max_rules_vis)
            .enumerate()
        {
            let item_r = RectF::new(
                list_box.x + 8.0,
                list_box.y + 32.0 + (idx as f32) * 46.0,
                list_w - 16.0,
                42.0,
            );
            let is_sel = idx == self.selected_prereq_idx;
            let is_hov = item_r.contains(mx, my);
            if is_sel {
                self.renderer
                    .fill_rounded_rect(item_r, 8.0, pal.primary_container);
                self.renderer
                    .draw_rounded_rect(item_r, 8.0, 1.2, pal.primary);
            } else if is_hov {
                self.renderer
                    .fill_rounded_rect(item_r, 8.0, rgba(0x6366F1, 0.08));
            }

            // Checkbox indicator on left of item
            let chk_box = RectF::new(item_r.x + 8.0, item_r.y + 12.0, 18.0, 18.0);
            if rule.enabled {
                self.renderer.fill_rounded_rect(chk_box, 4.0, pal.primary);
                self.renderer.draw_icon(
                    VectorIcon::Check,
                    chk_box.x + 9.0,
                    chk_box.y + 9.0,
                    11.0,
                    rgb(0xFFFFFF),
                );
            } else {
                self.renderer
                    .draw_rounded_rect(chk_box, 4.0, 1.2, pal.outline);
            }

            self.renderer.draw_text(
                &rule.name,
                &self.renderer.font_body_bold,
                RectF::new(item_r.x + 30.0, item_r.y + 4.0, item_r.w - 34.0, 18.0),
                if is_sel {
                    pal.on_primary_container
                } else {
                    pal.text_primary
                },
                0,
                1,
                false,
            );

            let local_ok = evaluate_prereq_condition(&rule.condition, None);
            let sub_tag = format!(
                "{} · {}",
                rule.condition.short_tag(),
                if local_ok { "本机已满足" } else { "本机未满足" }
            );
            self.renderer.draw_text(
                &sub_tag,
                &self.renderer.font_caption,
                RectF::new(item_r.x + 30.0, item_r.y + 22.0, item_r.w - 34.0, 16.0),
                if local_ok { pal.success } else { pal.warning },
                0,
                1,
                false,
            );
        }

        let btn_del_rule = RectF::new(
            list_box.x + 10.0,
            list_box.y + list_box.h - 42.0,
            list_w - 20.0,
            32.0,
        );
        self.renderer.draw_button(
            btn_del_rule,
            "删除选中规则",
            None,
            ButtonVariant::Outlined,
            btn_del_rule.contains(mx, my),
            !self.config.prerequisites.is_empty(),
            &pal,
            &self.ripples,
        );

        // Right Rule Editor Panel (x = x0 + 230.0, y = card.y + 128.0, w = w_full - 230.0, h = 368.0)
        let right_x = x0 + 230.0;
        let right_w = w_full - 230.0;
        let edit_box = RectF::new(right_x, card.y + 128.0, right_w, 368.0);
        self.renderer
            .fill_rounded_rect(edit_box, 10.0, pal.surface_variant);
        self.renderer
            .draw_rounded_rect(edit_box, 10.0, 1.0, pal.outline_subtle);

        let Some(rule) = self.config.prerequisites.get(self.selected_prereq_idx) else {
            self.renderer.draw_text(
                "当前未配置任何前置环境检测规则。\n点击上方预设按钮可一键添加 VC++ 运行库、注册表键值或文件/PE版本检测。",
                &self.renderer.font_body,
                edit_box.inset(24.0, 120.0),
                pal.text_muted,
                1,
                1,
                true,
            );
            self.draw_card_footer(
                card,
                "提示：随包运行库仅在目标机器检测未通过时解压至 %TEMP% 执行并自动清理",
                "下一步：构建安装包",
                Some(VectorIcon::Rocket),
            );
            return;
        };

        let ex = edit_box.x + 16.0;
        let ew = edit_box.w - 32.0;

        // Row 1 (y = card.y + 152.0): Rule Name + Condition Kind Dropdown
        let r_rname = RectF::new(ex, card.y + 152.0, ew - 178.0, 34.0);
        self.renderer.draw_text_field_with_sel(
            r_rname,
            "规则名称 (安装进度展示)",
            &rule.name,
            self.focused_field == FocusedField::PrereqName,
            r_rname.contains(mx, my),
            self.caret_visible,
            &self.text_input,
            &pal,
        );
        let dd_cond_kind = RectF::new(ex + ew - 168.0, card.y + 152.0, 168.0, 34.0);
        let cond_kind_label = match rule.condition {
            PrereqCondition::Registry { .. } => "注册表键值检测",
            PrereqCondition::File { .. } => "文件 / PE版本",
        };
        self.renderer.draw_dropdown(
            dd_cond_kind,
            "检测条件类型",
            cond_kind_label,
            self.open_dropdown == Some(PrereqDropdown::ConditionKind),
            dd_cond_kind.contains(mx, my),
            &pal,
        );

        // Row 2 & Row 3: Condition Details
        match &rule.condition {
            PrereqCondition::Registry {
                root,
                subkey,
                value_name,
                arch_view,
                operator,
                expected,
            } => {
                // Row 2 (y = card.y + 210.0): Hive Dropdown + Arch Dropdown + Key Path
                let dd_hive = RectF::new(ex, card.y + 210.0, 86.0, 34.0);
                self.renderer.draw_dropdown(
                    dd_hive,
                    "注册表根键",
                    root.label(),
                    self.open_dropdown == Some(PrereqDropdown::RegHive),
                    dd_hive.contains(mx, my),
                    &pal,
                );
                let dd_arch = RectF::new(ex + 94.0, card.y + 210.0, 102.0, 34.0);
                self.renderer.draw_dropdown(
                    dd_arch,
                    "注册表视图",
                    arch_view.label(),
                    self.open_dropdown == Some(PrereqDropdown::RegArch),
                    dd_arch.contains(mx, my),
                    &pal,
                );
                let r_kpath = RectF::new(ex + 204.0, card.y + 210.0, ew - 204.0, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_kpath,
                    "注册表子键路径 (Subkey)",
                    subkey,
                    self.focused_field == FocusedField::PrereqTarget,
                    r_kpath.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );

                // Row 3 (y = card.y + 268.0): Value Name + Operator Dropdown + Expected Value
                let r_vname = RectF::new(ex, card.y + 268.0, 130.0, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_vname,
                    "值名称 (空=仅测键)",
                    value_name,
                    self.focused_field == FocusedField::PrereqValueName,
                    r_vname.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );
                let dd_op = RectF::new(ex + 138.0, card.y + 268.0, 122.0, 34.0);
                self.renderer.draw_dropdown(
                    dd_op,
                    "比较条件",
                    operator.label(),
                    self.open_dropdown == Some(PrereqDropdown::Operator),
                    dd_op.contains(mx, my),
                    &pal,
                );
                let r_exp = RectF::new(ex + 268.0, card.y + 268.0, ew - 268.0, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_exp,
                    "期望值 / 版本",
                    expected,
                    self.focused_field == FocusedField::PrereqExpected,
                    r_exp.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );
            }
            PrereqCondition::File {
                path,
                operator,
                expected_version,
            } => {
                // Row 2 (y = card.y + 210.0): File Path
                let r_fpath = RectF::new(ex, card.y + 210.0, ew, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_fpath,
                    "目标文件路径 (支持 %SystemRoot% / %PROGRAMFILES% / %INSTALL_DIR%)",
                    path,
                    self.focused_field == FocusedField::PrereqTarget,
                    r_fpath.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );

                // Row 3 (y = card.y + 268.0): Operator Dropdown + Expected Version
                let dd_op = RectF::new(ex, card.y + 268.0, 146.0, 34.0);
                let file_op_label = match operator {
                    CheckOperator::Exists => "文件存在",
                    CheckOperator::GreaterOrEqual => "PE版本 >=",
                    CheckOperator::Equals => "PE版本 ==",
                    CheckOperator::NotExists => "文件不存在",
                    other => other.label(),
                };
                self.renderer.draw_dropdown(
                    dd_op,
                    "检测方式",
                    file_op_label,
                    self.open_dropdown == Some(PrereqDropdown::Operator),
                    dd_op.contains(mx, my),
                    &pal,
                );
                let r_exp = RectF::new(ex + 156.0, card.y + 268.0, ew - 156.0, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_exp,
                    "期望 PE 版本号 (如 14.38.0.0，仅测存在留空)",
                    expected_version,
                    self.focused_field == FocusedField::PrereqExpected,
                    r_exp.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );
            }
        }

        // Row 4 (y = card.y + 328.0): Action Type Dropdown
        let dd_act_kind = RectF::new(ex, card.y + 328.0, ew, 34.0);
        let cur_act_label = match rule.action {
            PrereqAction::RunBundledExe { .. } => {
                "1. 随包打包并运行安装器 (解压至 %TEMP% 执行并自动清理)"
            }
            PrereqAction::RunCommand { .. } => {
                "2. 执行系统命令或脚本 (支持 cmd / powershell / %INSTALL_DIR%)"
            }
            PrereqAction::PromptAndAbort { .. } => {
                "3. 弹窗提示用户并中止安装 (可选引导打开下载链接)"
            }
        };
        self.renderer.draw_dropdown(
            dd_act_kind,
            "检测未通过时的执行动作",
            cur_act_label,
            self.open_dropdown == Some(PrereqDropdown::ActionKind),
            dd_act_kind.contains(mx, my),
            &pal,
        );

        // Row 5 (y = card.y + 388.0) & Row 6 (y = card.y + 448.0): Action Fields
        match &rule.action {
            PrereqAction::RunBundledExe {
                source_file,
                args,
                elevate,
                ..
            } => {
                let r_act1 = RectF::new(ex, card.y + 388.0, ew - 96.0, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_act1,
                    "随包打包的安装器路径 (如 vc_redist.x64.exe)",
                    source_file,
                    self.focused_field == FocusedField::PrereqActionPrimary,
                    r_act1.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );
                let btn_pick_redist = RectF::new(ex + ew - 88.0, card.y + 388.0, 88.0, 34.0);
                self.renderer.draw_button(
                    btn_pick_redist,
                    "选择exe",
                    Some(VectorIcon::Folder),
                    ButtonVariant::Outlined,
                    btn_pick_redist.contains(mx, my),
                    true,
                    &pal,
                    &self.ripples,
                );

                let r_act2 = RectF::new(ex, card.y + 448.0, ew - 134.0, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_act2,
                    "静默安装参数 (如 /install /quiet /norestart)",
                    args,
                    self.focused_field == FocusedField::PrereqActionSecondary,
                    r_act2.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );
                let dd_elev = RectF::new(ex + ew - 126.0, card.y + 448.0, 126.0, 34.0);
                self.renderer.draw_dropdown(
                    dd_elev,
                    "权限模式",
                    if *elevate {
                        "UAC 提权运行"
                    } else {
                        "普通权限运行"
                    },
                    self.open_dropdown == Some(PrereqDropdown::ElevateMode),
                    dd_elev.contains(mx, my),
                    &pal,
                );
            }
            PrereqAction::RunCommand {
                program,
                args,
                elevate,
                ..
            } => {
                let r_act1 = RectF::new(ex, card.y + 388.0, ew, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_act1,
                    "执行程序路径 (支持 cmd / powershell / %INSTALL_DIR%)",
                    program,
                    self.focused_field == FocusedField::PrereqActionPrimary,
                    r_act1.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );

                let r_act2 = RectF::new(ex, card.y + 448.0, ew - 134.0, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_act2,
                    "命令行参数 (Arguments)",
                    args,
                    self.focused_field == FocusedField::PrereqActionSecondary,
                    r_act2.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );
                let dd_elev = RectF::new(ex + ew - 126.0, card.y + 448.0, 126.0, 34.0);
                self.renderer.draw_dropdown(
                    dd_elev,
                    "权限模式",
                    if *elevate {
                        "UAC 提权运行"
                    } else {
                        "普通权限运行"
                    },
                    self.open_dropdown == Some(PrereqDropdown::ElevateMode),
                    dd_elev.contains(mx, my),
                    &pal,
                );
            }
            PrereqAction::PromptAndAbort {
                message,
                download_url,
            } => {
                let r_act1 = RectF::new(ex, card.y + 388.0, ew, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_act1,
                    "检测未通过时的弹窗警告文本 (Message)",
                    message,
                    self.focused_field == FocusedField::PrereqActionPrimary,
                    r_act1.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );
                let r_act2 = RectF::new(ex, card.y + 448.0, ew, 34.0);
                self.renderer.draw_text_field_with_sel(
                    r_act2,
                    "引导用户打开的下载链接 URL (可选，留空仅提示)",
                    download_url,
                    self.focused_field == FocusedField::PrereqActionSecondary,
                    r_act2.contains(mx, my),
                    self.caret_visible,
                    &self.text_input,
                    &pal,
                );
            }
        }

        self.draw_card_footer(
            card,
            "提示：随包运行库仅在目标机器检测未通过时解压至 %TEMP% 执行并自动清理",
            "下一步：构建安装包",
            Some(VectorIcon::Rocket),
        );

        // Render active floating dropdown menu overlay on top of everything
        if let Some(open_dd) = self.open_dropdown {
            self.draw_prereq_dropdown_overlay(open_dd, rule, ex, ew, card.y);
        }
    }

    pub(crate) fn draw_prereq_dropdown_overlay(
        &self,
        open_dd: PrereqDropdown,
        rule: &PrerequisiteRule,
        ex: f32,
        ew: f32,
        card_y: f32,
    ) {
        let mx = self.mouse_x;
        let my = self.mouse_y;
        let w = self.width_dip;
        let h = self.height_dip;
        let pal = self.palette;

        match open_dd {
            PrereqDropdown::ConditionKind => {
                let trig = RectF::new(ex + ew - 168.0, card_y + 152.0, 168.0, 34.0);
                let items = ["注册表键值检测", "文件 / PE版本"];
                let sel = match rule.condition {
                    PrereqCondition::Registry { .. } => 0,
                    PrereqCondition::File { .. } => 1,
                };
                self.renderer
                    .draw_dropdown_menu(trig, &items, sel, mx, my, w, h, &pal);
            }
            PrereqDropdown::RegHive => {
                if let PrereqCondition::Registry { root, .. } = &rule.condition {
                    let trig = RectF::new(ex, card_y + 210.0, 86.0, 34.0);
                    let items = [
                        "HKLM (本机系统)",
                        "HKCU (当前用户)",
                        "HKCR (类与关联)",
                        "HKU (所有用户)",
                    ];
                    let sel = match root {
                        RegHiveRoot::Hklm => 0,
                        RegHiveRoot::Hkcu => 1,
                        RegHiveRoot::Hkcr => 2,
                        RegHiveRoot::Hku => 3,
                    };
                    self.renderer
                        .draw_dropdown_menu(trig, &items, sel, mx, my, w, h, &pal);
                }
            }
            PrereqDropdown::RegArch => {
                if let PrereqCondition::Registry { arch_view, .. } = &rule.condition {
                    let trig = RectF::new(ex + 94.0, card_y + 210.0, 102.0, 34.0);
                    let items = ["默认视图", "64位视图 (x64)", "32位视图 (x86)"];
                    let sel = match arch_view {
                        RegArchView::Default => 0,
                        RegArchView::X64 => 1,
                        RegArchView::X86 => 2,
                    };
                    self.renderer
                        .draw_dropdown_menu(trig, &items, sel, mx, my, w, h, &pal);
                }
            }
            PrereqDropdown::Operator => match &rule.condition {
                PrereqCondition::Registry { operator, .. } => {
                    let trig = RectF::new(ex + 138.0, card_y + 268.0, 122.0, 34.0);
                    let items = [
                        "存在",
                        "版本/数值 >=",
                        "等于 (==)",
                        "包含文本",
                        "不等于 (!=)",
                        "不存在",
                    ];
                    let sel = match operator {
                        CheckOperator::Exists => 0,
                        CheckOperator::GreaterOrEqual => 1,
                        CheckOperator::Equals => 2,
                        CheckOperator::Contains => 3,
                        CheckOperator::NotEquals => 4,
                        CheckOperator::NotExists => 5,
                    };
                    self.renderer
                        .draw_dropdown_menu(trig, &items, sel, mx, my, w, h, &pal);
                }
                PrereqCondition::File { operator, .. } => {
                    let trig = RectF::new(ex, card_y + 268.0, 146.0, 34.0);
                    let items = ["文件存在", "PE版本 >=", "PE版本 ==", "文件不存在"];
                    let sel = match operator {
                        CheckOperator::Exists => 0,
                        CheckOperator::GreaterOrEqual => 1,
                        CheckOperator::Equals => 2,
                        CheckOperator::NotExists => 3,
                        _ => 1,
                    };
                    self.renderer
                        .draw_dropdown_menu(trig, &items, sel, mx, my, w, h, &pal);
                }
            },
            PrereqDropdown::ActionKind => {
                let trig = RectF::new(ex, card_y + 328.0, ew, 34.0);
                let items = [
                    "1. 随包打包并运行安装器 (解压至 %TEMP% 执行并自动清理)",
                    "2. 执行系统命令或安装目录脚本 (支持 cmd / powershell / %INSTALL_DIR%)",
                    "3. 弹窗提示用户并中止安装 (可选引导打开下载链接)",
                ];
                let sel = match rule.action {
                    PrereqAction::RunBundledExe { .. } => 0,
                    PrereqAction::RunCommand { .. } => 1,
                    PrereqAction::PromptAndAbort { .. } => 2,
                };
                self.renderer
                    .draw_dropdown_menu(trig, &items, sel, mx, my, w, h, &pal);
            }
            PrereqDropdown::ElevateMode => {
                let cur_elev = match &rule.action {
                    PrereqAction::RunBundledExe { elevate, .. }
                    | PrereqAction::RunCommand { elevate, .. } => Some(*elevate),
                    PrereqAction::PromptAndAbort { .. } => None,
                };
                if let Some(elevate) = cur_elev {
                    let trig = RectF::new(ex + ew - 126.0, card_y + 448.0, 126.0, 34.0);
                    let items = ["UAC 提权运行 (管理员)", "普通权限运行 (当前用户)"];
                    let sel = if elevate { 0 } else { 1 };
                    self.renderer
                        .draw_dropdown_menu(trig, &items, sel, mx, my, w, h, &pal);
                }
            }
        }
    }

    pub(crate) fn try_click_prereq_dropdown_menu(
        &mut self,
        open_dd: PrereqDropdown,
        x: f32,
        y: f32,
    ) -> bool {
        let card = self.content_card_rect();
        let x0 = card.x + 28.0;
        let w_full = card.w - 56.0;
        let right_x = x0 + 230.0;
        let right_w = w_full - 230.0;
        let ex = right_x + 16.0;
        let ew = right_w - 32.0;
        let w = self.width_dip;
        let h = self.height_dip;

        let Some(rule_ref) = self.config.prerequisites.get(self.selected_prereq_idx) else {
            return false;
        };

        let (trig, items_vec): (RectF, Vec<&str>) = match open_dd {
            PrereqDropdown::ConditionKind => (
                RectF::new(ex + ew - 168.0, card.y + 152.0, 168.0, 34.0),
                vec!["注册表键值检测", "文件 / PE版本"],
            ),
            PrereqDropdown::RegHive => (
                RectF::new(ex, card.y + 210.0, 86.0, 34.0),
                vec![
                    "HKLM (本机系统)",
                    "HKCU (当前用户)",
                    "HKCR (类与关联)",
                    "HKU (所有用户)",
                ],
            ),
            PrereqDropdown::RegArch => (
                RectF::new(ex + 94.0, card.y + 210.0, 102.0, 34.0),
                vec!["默认视图", "64位视图 (x64)", "32位视图 (x86)"],
            ),
            PrereqDropdown::Operator => match &rule_ref.condition {
                PrereqCondition::Registry { .. } => (
                    RectF::new(ex + 138.0, card.y + 268.0, 122.0, 34.0),
                    vec![
                        "存在",
                        "版本/数值 >=",
                        "等于 (==)",
                        "包含文本",
                        "不等于 (!=)",
                        "不存在",
                    ],
                ),
                PrereqCondition::File { .. } => (
                    RectF::new(ex, card.y + 268.0, 146.0, 34.0),
                    vec!["文件存在", "PE版本 >=", "PE版本 ==", "文件不存在"],
                ),
            },
            PrereqDropdown::ActionKind => (
                RectF::new(ex, card.y + 328.0, ew, 34.0),
                vec![
                    "1. 随包打包并运行安装器 (解压至 %TEMP% 执行并自动清理)",
                    "2. 执行系统命令或安装目录脚本 (支持 cmd / powershell / %INSTALL_DIR%)",
                    "3. 弹窗提示用户并中止安装 (可选引导打开下载链接)",
                ],
            ),
            PrereqDropdown::ElevateMode => (
                RectF::new(ex + ew - 126.0, card.y + 448.0, 126.0, 34.0),
                vec!["UAC 提权运行 (管理员)", "普通权限运行 (当前用户)"],
            ),
        };

        if trig.contains(x, y) {
            return true;
        }

        let menu_rect = self.renderer.dropdown_menu_rect(trig, &items_vec, w, h);
        if !menu_rect.contains(x, y) {
            return false;
        }

        let item_h = 32.0;
        let rel_y = (y - (menu_rect.y + 4.0)).max(0.0);
        let picked_idx = (rel_y / item_h).floor() as usize;
        if picked_idx >= items_vec.len() {
            return true;
        }

        if let Some(rule) = self.config.prerequisites.get_mut(self.selected_prereq_idx) {
            match open_dd {
                PrereqDropdown::ConditionKind => {
                    if picked_idx == 0 && matches!(rule.condition, PrereqCondition::File { .. }) {
                        rule.condition = PrereqCondition::Registry {
                            root: RegHiveRoot::Hklm,
                            subkey: "SOFTWARE\\Microsoft\\VisualStudio\\14.0\\VC\\Runtimes\\x64"
                                .to_string(),
                            value_name: "Installed".to_string(),
                            arch_view: RegArchView::X64,
                            operator: CheckOperator::GreaterOrEqual,
                            expected: "1".to_string(),
                        };
                    } else if picked_idx == 1
                        && matches!(rule.condition, PrereqCondition::Registry { .. })
                    {
                        rule.condition = PrereqCondition::File {
                            path: "%SystemRoot%\\System32\\vcruntime140.dll".to_string(),
                            operator: CheckOperator::GreaterOrEqual,
                            expected_version: "14.38.0.0".to_string(),
                        };
                    }
                }
                PrereqDropdown::RegHive => {
                    if let PrereqCondition::Registry { root, .. } = &mut rule.condition {
                        *root = match picked_idx {
                            0 => RegHiveRoot::Hklm,
                            1 => RegHiveRoot::Hkcu,
                            2 => RegHiveRoot::Hkcr,
                            _ => RegHiveRoot::Hku,
                        };
                    }
                }
                PrereqDropdown::RegArch => {
                    if let PrereqCondition::Registry { arch_view, .. } = &mut rule.condition {
                        *arch_view = match picked_idx {
                            0 => RegArchView::Default,
                            1 => RegArchView::X64,
                            _ => RegArchView::X86,
                        };
                    }
                }
                PrereqDropdown::Operator => match &mut rule.condition {
                    PrereqCondition::Registry { operator, .. } => {
                        *operator = match picked_idx {
                            0 => CheckOperator::Exists,
                            1 => CheckOperator::GreaterOrEqual,
                            2 => CheckOperator::Equals,
                            3 => CheckOperator::Contains,
                            4 => CheckOperator::NotEquals,
                            _ => CheckOperator::NotExists,
                        };
                    }
                    PrereqCondition::File { operator, .. } => {
                        *operator = match picked_idx {
                            0 => CheckOperator::Exists,
                            1 => CheckOperator::GreaterOrEqual,
                            2 => CheckOperator::Equals,
                            _ => CheckOperator::NotExists,
                        };
                    }
                },
                PrereqDropdown::ActionKind => {
                    let cur_kind = match rule.action {
                        PrereqAction::RunBundledExe { .. } => 0,
                        PrereqAction::RunCommand { .. } => 1,
                        PrereqAction::PromptAndAbort { .. } => 2,
                    };
                    if picked_idx != cur_kind {
                        rule.action = match picked_idx {
                            0 => PrereqAction::RunBundledExe {
                                source_file: "vc_redist.x64.exe".to_string(),
                                bundled_rel_path: String::new(),
                                args: "/install /quiet /norestart".to_string(),
                                elevate: true,
                                wait: true,
                                success_exit_codes: vec![0, 1638, 3010],
                            },
                            1 => PrereqAction::RunCommand {
                                program: "cmd.exe".to_string(),
                                args: "/C echo Configuring prerequisite...".to_string(),
                                elevate: false,
                                wait: true,
                                success_exit_codes: vec![0],
                            },
                            _ => PrereqAction::PromptAndAbort {
                                message: "检测到系统缺少必要的运行库组件，请先安装该运行库后再运行本安装程序。"
                                    .to_string(),
                                download_url:
                                    "https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist"
                                        .to_string(),
                            },
                        };
                    }
                }
                PrereqDropdown::ElevateMode => match &mut rule.action {
                    PrereqAction::RunBundledExe { elevate, .. }
                    | PrereqAction::RunCommand { elevate, .. } => {
                        *elevate = picked_idx == 0;
                    }
                    PrereqAction::PromptAndAbort { .. } => {}
                },
            }
        }
        true
    }

    pub(crate) fn handle_prereq_tab_click(
        &mut self,
        x: f32,
        y: f32,
        card: RectF,
        x0: f32,
        w_full: f32,
        btn_next: RectF,
        prev_focus: FocusedField,
    ) {
        // 1. Top Preset Bar
        let preset_w = (w_full - 30.0) / 4.0;
        for i in 0..4usize {
            let pr = RectF::new(
                x0 + (i as f32) * (preset_w + 10.0),
                card.y + 84.0,
                preset_w,
                34.0,
            );
            if pr.contains(x, y) {
                let new_rule = match i {
                    0 => PrerequisiteRule::preset_vc_redist_x64(),
                    1 => PrerequisiteRule::preset_vc_redist_x86(),
                    2 => PrerequisiteRule::preset_custom_registry(),
                    _ => PrerequisiteRule::preset_custom_file_version(),
                };
                self.config.prerequisites.push(new_rule);
                self.selected_prereq_idx = self.config.prerequisites.len().saturating_sub(1);
                self.add_ripple(x, y, pr, rgba(0x6366F1, 0.25));
                return;
            }
        }

        // 2. Left Rule List & Delete button
        let list_w = 216.0;
        let list_box = RectF::new(x0, card.y + 128.0, list_w, 368.0);
        for idx in 0..self.config.prerequisites.len().min(6) {
            let item_r = RectF::new(
                list_box.x + 8.0,
                list_box.y + 32.0 + (idx as f32) * 46.0,
                list_w - 16.0,
                42.0,
            );
            if item_r.contains(x, y) {
                let chk_area = RectF::new(item_r.x, item_r.y, 32.0, item_r.h);
                if chk_area.contains(x, y) {
                    if let Some(r) = self.config.prerequisites.get_mut(idx) {
                        r.enabled = !r.enabled;
                    }
                }
                self.selected_prereq_idx = idx;
                return;
            }
        }

        let btn_del_rule = RectF::new(
            list_box.x + 10.0,
            list_box.y + list_box.h - 42.0,
            list_w - 20.0,
            32.0,
        );
        if btn_del_rule.contains(x, y) && !self.config.prerequisites.is_empty() {
            let rem_idx = self
                .selected_prereq_idx
                .min(self.config.prerequisites.len() - 1);
            self.config.prerequisites.remove(rem_idx);
            if self.selected_prereq_idx >= self.config.prerequisites.len()
                && !self.config.prerequisites.is_empty()
            {
                self.selected_prereq_idx = self.config.prerequisites.len() - 1;
            }
            return;
        }

        if btn_next.contains(x, y) {
            self.active_tab = StudioTab::BuildAndTest;
            self.rescan_source_dir();
            return;
        }

        // 3. Right Rule Editor Controls
        let right_x = x0 + 230.0;
        let right_w = w_full - 230.0;
        let ex = right_x + 16.0;
        let ew = right_w - 32.0;

        if self.config.prerequisites.get(self.selected_prereq_idx).is_none() {
            return;
        }

        let r_rname = RectF::new(ex, card.y + 152.0, ew - 178.0, 34.0);
        let dd_cond_kind = RectF::new(ex + ew - 168.0, card.y + 152.0, 168.0, 34.0);
        let dd_act_kind = RectF::new(ex, card.y + 328.0, ew, 34.0);

        if r_rname.contains(x, y) {
            self.focused_field = prev_focus;
            self.click_text_field(FocusedField::PrereqName, r_rname, x);
        } else if dd_cond_kind.contains(x, y) {
            self.open_dropdown = Some(PrereqDropdown::ConditionKind);
        } else if dd_act_kind.contains(x, y) {
            self.open_dropdown = Some(PrereqDropdown::ActionKind);
        } else {
            let is_reg = matches!(
                self.config.prerequisites[self.selected_prereq_idx].condition,
                PrereqCondition::Registry { .. }
            );
            if is_reg {
                let dd_hive = RectF::new(ex, card.y + 210.0, 86.0, 34.0);
                let dd_arch = RectF::new(ex + 94.0, card.y + 210.0, 102.0, 34.0);
                let r_kpath = RectF::new(ex + 204.0, card.y + 210.0, ew - 204.0, 34.0);
                let r_vname = RectF::new(ex, card.y + 268.0, 130.0, 34.0);
                let dd_op = RectF::new(ex + 138.0, card.y + 268.0, 122.0, 34.0);
                let r_exp = RectF::new(ex + 268.0, card.y + 268.0, ew - 268.0, 34.0);

                if dd_hive.contains(x, y) {
                    self.open_dropdown = Some(PrereqDropdown::RegHive);
                } else if dd_arch.contains(x, y) {
                    self.open_dropdown = Some(PrereqDropdown::RegArch);
                } else if r_kpath.contains(x, y) {
                    self.focused_field = prev_focus;
                    self.click_text_field(FocusedField::PrereqTarget, r_kpath, x);
                } else if r_vname.contains(x, y) {
                    self.focused_field = prev_focus;
                    self.click_text_field(FocusedField::PrereqValueName, r_vname, x);
                } else if dd_op.contains(x, y) {
                    self.open_dropdown = Some(PrereqDropdown::Operator);
                } else if r_exp.contains(x, y) {
                    self.focused_field = prev_focus;
                    self.click_text_field(FocusedField::PrereqExpected, r_exp, x);
                }
            } else {
                let r_fpath = RectF::new(ex, card.y + 210.0, ew, 34.0);
                let dd_op = RectF::new(ex, card.y + 268.0, 146.0, 34.0);
                let r_exp = RectF::new(ex + 156.0, card.y + 268.0, ew - 156.0, 34.0);

                if r_fpath.contains(x, y) {
                    self.focused_field = prev_focus;
                    self.click_text_field(FocusedField::PrereqTarget, r_fpath, x);
                } else if dd_op.contains(x, y) {
                    self.open_dropdown = Some(PrereqDropdown::Operator);
                } else if r_exp.contains(x, y) {
                    self.focused_field = prev_focus;
                    self.click_text_field(FocusedField::PrereqExpected, r_exp, x);
                }
            }

            // Row 5 & Row 6: Action Fields
            let act_kind = match self.config.prerequisites[self.selected_prereq_idx].action {
                PrereqAction::RunBundledExe { .. } => 0,
                PrereqAction::RunCommand { .. } => 1,
                PrereqAction::PromptAndAbort { .. } => 2,
            };
            match act_kind {
                0 => {
                    let r_act1 = RectF::new(ex, card.y + 388.0, ew - 96.0, 34.0);
                    let btn_pick_redist = RectF::new(ex + ew - 88.0, card.y + 388.0, 88.0, 34.0);
                    let r_act2 = RectF::new(ex, card.y + 448.0, ew - 134.0, 34.0);
                    let dd_elev = RectF::new(ex + ew - 126.0, card.y + 448.0, 126.0, 34.0);

                    if r_act1.contains(x, y) {
                        self.focused_field = prev_focus;
                        self.click_text_field(FocusedField::PrereqActionPrimary, r_act1, x);
                    } else if btn_pick_redist.contains(x, y) {
                        if let Some(picked) = pick_open_file_dialog(
                            self.hwnd,
                            "选择随包打包的前置组件安装程序 (.exe)",
                            "可执行安装程序 (*.exe)",
                            "*.exe",
                        ) {
                            if let Some(rule) = self
                                .config
                                .prerequisites
                                .get_mut(self.selected_prereq_idx)
                            {
                                if let PrereqAction::RunBundledExe { source_file, .. } =
                                    &mut rule.action
                                {
                                    *source_file = picked.to_string_lossy().to_string();
                                }
                            }
                        }
                    } else if r_act2.contains(x, y) {
                        self.focused_field = prev_focus;
                        self.click_text_field(FocusedField::PrereqActionSecondary, r_act2, x);
                    } else if dd_elev.contains(x, y) {
                        self.open_dropdown = Some(PrereqDropdown::ElevateMode);
                    }
                }
                1 => {
                    let r_act1 = RectF::new(ex, card.y + 388.0, ew, 34.0);
                    let r_act2 = RectF::new(ex, card.y + 448.0, ew - 134.0, 34.0);
                    let dd_elev = RectF::new(ex + ew - 126.0, card.y + 448.0, 126.0, 34.0);

                    if r_act1.contains(x, y) {
                        self.focused_field = prev_focus;
                        self.click_text_field(FocusedField::PrereqActionPrimary, r_act1, x);
                    } else if r_act2.contains(x, y) {
                        self.focused_field = prev_focus;
                        self.click_text_field(FocusedField::PrereqActionSecondary, r_act2, x);
                    } else if dd_elev.contains(x, y) {
                        self.open_dropdown = Some(PrereqDropdown::ElevateMode);
                    }
                }
                _ => {
                    let r_act1 = RectF::new(ex, card.y + 388.0, ew, 34.0);
                    let r_act2 = RectF::new(ex, card.y + 448.0, ew, 34.0);
                    if r_act1.contains(x, y) {
                        self.focused_field = prev_focus;
                        self.click_text_field(FocusedField::PrereqActionPrimary, r_act1, x);
                    } else if r_act2.contains(x, y) {
                        self.focused_field = prev_focus;
                        self.click_text_field(FocusedField::PrereqActionSecondary, r_act2, x);
                    }
                }
            }
        }
    }
}
