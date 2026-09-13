fn main() {
    match devbox_control_center_lib::bootstrap::run(std::env::args_os().skip(1).collect()) {
        Ok(result) => match serde_json::to_string(&result) {
            Ok(result) => println!("{result}"),
            Err(_) => std::process::exit(2),
        },
        Err(issue) => {
            eprintln!("{issue}");
            std::process::exit(2);
        }
    }
}
