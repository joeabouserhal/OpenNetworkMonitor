pub mod alerts;
pub mod app;
mod app_icon;
pub mod capture;
pub mod diagnostics;
pub mod hosts;
pub mod icons;
pub mod model;
pub mod notify;
pub mod startup;
pub mod store;
pub mod titlebar;
pub mod traffic;
pub mod tray;
pub mod tray_menu;
pub mod ui;
pub mod window_state;
pub mod worker;

#[cfg(test)]
mod release_tests {
    /// The executable carries two version numbers, one from Cargo and one in the Windows
    /// manifest. They are written by hand, so this keeps them from drifting apart.
    #[test]
    fn the_windows_manifest_version_matches_the_package_version() {
        let manifest = include_str!("../app.manifest");
        let expected = format!("version=\"{}.0\"", env!("CARGO_PKG_VERSION"));
        assert!(
            manifest.contains(&format!("name=\"OpenNetworkMonitor\" {expected}")),
            "app.manifest should declare {expected}"
        );
    }
}
