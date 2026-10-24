use serde::Deserialize;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowsPackage {
    DockerDesktop,
}
pub fn winget_arguments(package: WindowsPackage) -> [&'static str; 6] {
    match package {
        WindowsPackage::DockerDesktop => [
            "install",
            "--id",
            "Docker.DockerDesktop",
            "-e",
            "--source",
            "winget",
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_allowlisted_winget_packages_deserialize() {
        assert_eq!(
            serde_json::from_str::<WindowsPackage>("\"dockerDesktop\"").unwrap(),
            WindowsPackage::DockerDesktop
        );
        for raw in ["\"Docker.Other\"", "\"cmd.exe\"", "{\"id\":\"arbitrary\"}"] {
            assert!(serde_json::from_str::<WindowsPackage>(raw).is_err());
        }
    }
    #[test]
    fn winget_arguments_are_fixed() {
        assert_eq!(
            winget_arguments(WindowsPackage::DockerDesktop),
            [
                "install",
                "--id",
                "Docker.DockerDesktop",
                "-e",
                "--source",
                "winget"
            ]
        );
    }
}
