use super::*;

#[test]
fn test_draw_titlebar_header_renders() {
    let mut d2d = D2DRenderer::new().expect("D2D Init");
    d2d.resize(800, 46, 96);
    let pal = MaterialPalette::dark();
    unsafe {
        d2d.rt().BeginDraw();
        d2d.rt().Clear(Some(&pal.bg));
        d2d.draw_window_header(
            800.0,
            "Nyan Studio App v1.0.0 安装向导",
            "",
            &pal,
            false,
            false,
            false,
        );
        let _ = d2d.rt().EndDraw(None, None);
    }
    let out_dir = std::path::Path::new("target");
    let out_bmp = out_dir.join("test_titlebar.bmp");
    let _ = d2d.save_bmp(&out_bmp);
    assert!(out_bmp.exists());
}

#[test]
fn test_text_input_selection_and_editing() {
    let mut s = "Hello 世界".to_string();
    let mut st = TextInputState::default();
    st.select_all(&s);
    assert!(st.has_selection());
    assert_eq!(st.selected_text(&s), Some("Hello 世界"));

    st.insert_str(&mut s, "Nyan");
    assert_eq!(s, "Nyan");
    assert!(!st.has_selection());
    assert_eq!(st.caret_pos, 4);

    st.move_left(&s, true);
    st.move_left(&s, true);
    assert_eq!(st.selected_text(&s), Some("an"));

    st.backspace(&mut s);
    assert_eq!(s, "Ny");
}

#[test]
fn test_material_icons_svg_geometry_renders_all() {
    let mut d2d = D2DRenderer::new().expect("D2D Init");
    assert!(
        d2d.font_material_icon.is_some(),
        "In-memory MaterialIcons-Regular.ttf from material-icons crate should load into DirectWrite"
    );
    d2d.resize(512, 64, 96);
    let pal = MaterialPalette::light();
    let all_icons = [
        VectorIcon::Info,
        VectorIcon::Folder,
        VectorIcon::Document,
        VectorIcon::Rocket,
        VectorIcon::Check,
        VectorIcon::Shield,
        VectorIcon::HardDrive,
        VectorIcon::Trash,
        VectorIcon::Sun,
        VectorIcon::Moon,
        VectorIcon::Minimize,
        VectorIcon::Close,
        VectorIcon::Sparkles,
        VectorIcon::Play,
        VectorIcon::ChevronDown,
        VectorIcon::ChevronUp,
    ];
    unsafe {
        d2d.rt().BeginDraw();
        d2d.rt().Clear(Some(&pal.surface));
        for (i, icon) in all_icons.iter().enumerate() {
            assert!(!icon.material_name().is_empty());
            assert!(icon.material_char() as u32 >= 0xE000);
            let geom = build_material_icon_geometry(
                &d2d.d2d_factory,
                icon.material_svg_path(),
                20.0 + (i as f32) * 30.0,
                32.0,
                20.0,
            );
            assert!(geom.is_ok(), "Failed to build MaterialIcon {:?}", icon);
            d2d.draw_icon(*icon, 20.0 + (i as f32) * 30.0, 32.0, 20.0, pal.primary);
        }
        let _ = d2d.rt().EndDraw(None, None);
    }
}
