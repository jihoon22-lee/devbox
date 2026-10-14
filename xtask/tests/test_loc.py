import importlib.util
from pathlib import Path
import unittest

MODULE = Path(__file__).resolve().parents[1] / 'loc.py'


class SourceCountTests(unittest.TestCase):
    def strip(self, text):
        spec = importlib.util.spec_from_file_location('devbox_loc', MODULE)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module.without_rust_tests(text)

    def test_leading_middle_trailing_and_external_test_items_preserve_product(self):
        text = '''#[cfg(test)]
mod first { fn test() {} }
fn before() {}
#[cfg(all(unix, test))]
#[path = "fixture.rs"]
mod external;
#[cfg(test)]
mod middle { mod nested { fn test() {} } }
fn after() {}
#[cfg(test)]
fn last() {}
'''
        output = self.strip(text)
        self.assertIn('fn before()', output)
        self.assertIn('fn after()', output)
        for removed in ['first', 'external', 'middle', 'last']:
            self.assertNotIn(removed, output)

    def test_literals_comments_and_combined_cfg_are_not_mistaken_for_test_items(self):
        text = '''const KEEP: &str = r###"#[cfg(test)] mod fake { }"###;
// #[cfg(test)] fn fake() {}
/* nested /* #[cfg(test)] */ comment */
#[cfg(test)]
mod tests {
  const TEXT: &str = r#" } ] \" "#;
  fn f() { let brace = '}'; let unicode = '\\u{7b}'; }
}
#[cfg(any(test, windows))]
fn native_or_test() {}
#[cfg(all(not(test), unix))]
fn product() {}
#[test]
fn standalone_test() {}
'''
        output = self.strip(text)
        self.assertIn('const KEEP:', output)
        self.assertIn('native_or_test', output)
        self.assertIn('fn product()', output)
        self.assertNotIn('mod tests', output)
        self.assertNotIn('standalone_test', output)

    def test_actual_files_keep_production_after_test_modules(self):
        root = MODULE.parent.parent
        for relative, token in [
            ('crates/cli/src/setup.rs', 'pub async fn'),
            ('crates/win/src/taskbar.rs', 'mod native'),
            ('crates/agents/src/store.rs', 'pub const MIGRATION_2'),
        ]:
            output = self.strip((root / relative).read_text())
            self.assertIn(token, output, relative)
            self.assertNotIn('mod tests {', output, relative)


if __name__ == '__main__':
    unittest.main()
