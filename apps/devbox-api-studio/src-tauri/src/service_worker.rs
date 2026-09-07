//! Runtime-managed mock server: explicit profile argv, no product UI/IPC/session.
pub fn argument(args: &[String]) -> Result<Option<String>, String> {
    let value = webhook_core::core::service_profile::parse_service_profile_argv(args)?;
    if value.is_none()
        && args
            .iter()
            .skip(1)
            .any(|arg| arg.starts_with("--service-profile"))
    {
        return Err("service_profile_arguments_invalid".into());
    }
    Ok(value)
}
pub fn run(id: String, mut context: tauri::Context<tauri::Wry>) -> tauri::Result<()> {
    // A debug executable can inherit or allocate a console even without a
    // WebView. Explicit service mode owns no interactive console lifetime.
    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::System::Console::FreeConsole();
    }
    product_shell_tauri::isolate_installation(&mut context)?;
    // Match Tauri's desktop app_local_data_dir without constructing an App.
    // The installation identifier is derived natively from this executable.
    let root = dirs::data_local_dir()
        .ok_or(tauri::Error::UnknownPath)?
        .join(&context.config().identifier)
        .join("webhooks");
    let worker = webhook_lab_lib::component::OwnedProfile::start(&root, &id)
        .map_err(std::io::Error::other)?;
    worker.wait().map_err(std::io::Error::other)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn service_mode_requires_the_exact_profile_pair_and_rejects_paths_or_other_modes() {
        let id = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            argument(&["app".into(), "--service-profile".into(), id.clone()]).unwrap(),
            Some(id.clone())
        );
        for args in [
            vec!["app", "--service-profile", "../profile"],
            vec!["app", "--service-profile=bad"],
            vec!["app", "--service-profile", &id, "--import-legacy"],
        ] {
            assert!(argument(&args.into_iter().map(str::to_string).collect::<Vec<_>>()).is_err());
        }
        assert_eq!(
            argument(&["app".into(), "--import-legacy".into()]).unwrap(),
            None
        );
    }
}
