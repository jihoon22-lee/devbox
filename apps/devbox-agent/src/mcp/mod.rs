pub mod host;
pub mod launcher;
pub mod notes;
pub mod owner;
pub mod route;
pub mod settings;
pub mod stdio;
pub fn run_stdio() -> i32 {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args != [std::ffi::OsString::from("--mcp-stdio")] {
        eprintln!("Devbox MCP arguments are invalid.");
        return 2;
    }
    #[cfg(windows)]
    let result = (|| -> Result<(), ()> {
        let image = std::env::current_exe().map_err(|_| ())?;
        devbox_filesystem::ensure_no_links(&image).map_err(|_| ())?;
        let image = image.canonicalize().map_err(|_| ())?;
        if !image.file_name().is_some_and(|name| {
            name.to_string_lossy()
                .eq_ignore_ascii_case("devbox-mcp.exe")
        }) {
            return Err(());
        }
        let root = image
            .parent()
            .and_then(|bin| bin.parent())
            .ok_or(())?
            .to_owned();
        suite_runtime::mcp_launcher::verify_image(&root, &image).map_err(|_| ())?;
        let host = host::AgentToolHost::new(root).map_err(|_| ())?;
        stdio::run(host, std::io::stdin().lock(), std::io::stdout().lock()).map_err(|_| ())
    })();
    #[cfg(not(windows))]
    let result: Result<(), ()> = Err(());
    if result.is_err() {
        eprintln!("Devbox MCP could not start from its installed launcher.");
        2
    } else {
        0
    }
}
