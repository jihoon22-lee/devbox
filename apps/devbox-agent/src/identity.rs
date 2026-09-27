pub fn pipe_name(suffix: &str) -> String {
    format!(r"\\.\pipe\devbox-agent-{suffix}")
}
#[cfg(test)]
mod tests {
    #[test]
    fn pipe_is_scoped_to_the_verified_installation_suffix() {
        assert_eq!(
            super::pipe_name("fixture"),
            r"\\.\pipe\devbox-agent-fixture"
        );
    }
}
