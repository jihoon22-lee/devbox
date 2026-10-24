//! Query text → one line-oriented matcher shared by code, file-name and note scans.
use crate::model::SearchError;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};

/// Compiled program bounds: a long or nested pattern is refused, not built.
pub const SIZE_LIMIT: usize = 1 << 20;
pub const DFA_SIZE_LIMIT: usize = 1 << 20;

/// `regex: false` matches `text` literally. Matches never span lines. A regex
/// syntax error reports its UTF-16 position in `text`; a pattern whose program
/// would exceed [`SIZE_LIMIT`] is refused at position 0.
pub fn compile(text: &str, regex: bool, case_sensitive: bool) -> Result<RegexMatcher, SearchError> {
    if regex {
        let ast = regex_syntax::ast::parse::Parser::new()
            .parse(text)
            .map_err(|e| invalid(text, e.span().start.offset))?;
        regex_syntax::hir::translate::TranslatorBuilder::new()
            .case_insensitive(!case_sensitive)
            .build()
            .translate(text, &ast)
            .map_err(|e| invalid(text, e.span().start.offset))?;
    }
    RegexMatcherBuilder::new()
        .case_insensitive(!case_sensitive)
        .fixed_strings(!regex)
        .line_terminator(Some(b'\n'))
        .size_limit(SIZE_LIMIT)
        .dfa_size_limit(DFA_SIZE_LIMIT)
        .build(text)
        .map_err(|_| {
            if regex {
                SearchError::InvalidRegex { position: 0 }
            } else {
                SearchError::InvalidInput
            }
        })
}

fn invalid(text: &str, byte: usize) -> SearchError {
    SearchError::InvalidRegex {
        position: utf16_at(text, byte),
    }
}

/// UTF-16 length of `text[..byte]` (`byte` clamped to a char boundary).
pub(crate) fn utf16_at(text: &str, byte: usize) -> u32 {
    let mut b = byte.min(text.len());
    while !text.is_char_boundary(b) {
        b -= 1;
    }
    text[..b].encode_utf16().count() as u32
}
