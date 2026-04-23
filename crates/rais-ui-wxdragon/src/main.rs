#[cfg(feature = "gui")]
mod wx_app;

#[cfg(feature = "gui")]
fn main() {
    wx_app::run();
}

#[cfg(not(feature = "gui"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = rais_ui_wxdragon::load_wizard_model(Default::default())?;
    println!("{}", model.window_title);
    println!("Current step: {:?}", model.current_step);
    println!("Targets: {}", model.target_rows.len());
    println!("Packages: {}", model.package_rows.len());
    println!("Build with `--features gui` to run the native wxDragon window.");
    Ok(())
}
