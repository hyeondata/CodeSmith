fn main() {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([980.0, 640.0]),
        ..Default::default()
    };
    if let Err(error) = eframe::run_native(
        "CodeSmith",
        options,
        Box::new(|cc| Ok(Box::new(codesmith_ui::CodeSmithApp::new(cc)))),
    ) {
        eprintln!("failed to start CodeSmith: {error}");
    }
}
