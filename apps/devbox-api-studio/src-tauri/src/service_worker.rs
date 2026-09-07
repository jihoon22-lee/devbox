//! Runtime-managed mock server: explicit profile argv, no product UI/IPC/session.
use tauri::Manager;
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
    context.config_mut().app.windows.clear();
    let app = tauri::Builder::default()
        .setup(move |app| {
            if !app.webview_windows().is_empty() {
                return Err(
                    std::io::Error::other("service worker must not create a webview").into(),
                );
            }
            // Same installation/profile namespace, independent process ownership.
            // No single-instance plugin, renderer, API state or global service owner.
            webhook_lab_lib::component::initialize(app.handle()).map_err(std::io::Error::other)?;
            webhook_lab_lib::component::start_owned_profile(app.handle(), &id)
                .map_err(std::io::Error::other)?;
            Ok(())
        })
        .build(context)?;
    app.run(|handle, event| match event {
        tauri::RunEvent::ExitRequested {
            api, code: None, ..
        } => api.prevent_exit(),
        tauri::RunEvent::Exit => {
            let _ = webhook_lab_lib::component::stop_owned_listener(handle);
        }
        _ => {}
    });
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
