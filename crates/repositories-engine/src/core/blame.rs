//! Pure porcelain blame parsing with a display line bound.
use serde::Serialize;
use std::collections::BTreeMap;
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct BlameLine {
    pub line: u32,
    pub commit: String,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct BlameCommit {
    pub author: String,
    pub author_time: i64,
    pub summary: String,
}
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Blame {
    pub file: String,
    pub lines: Vec<BlameLine>,
    pub commits: BTreeMap<String, BlameCommit>,
    pub truncated: bool,
}
const INVALID: &str = "blame_unavailable";
pub fn parse_porcelain(output: &str, max_lines: usize) -> Result<Blame, String> {
    let mut result = Blame {
        file: String::new(),
        lines: vec![],
        commits: BTreeMap::new(),
        truncated: false,
    };
    let mut records = output.split_terminator('\n');
    while let Some(header) = records.next() {
        if result.lines.len() >= max_lines {
            result.truncated = true;
            break;
        }
        let fields = header.split_whitespace().collect::<Vec<_>>();
        if !(3..=4).contains(&fields.len())
            || !matches!(fields[0].len(), 40 | 64)
            || !fields[0].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(INVALID.into());
        }
        let commit = fields[0].to_owned();
        for number in &fields[1..] {
            if number.parse::<u32>().ok().is_none_or(|n| n == 0) {
                return Err(INVALID.into());
            }
        }
        let line = fields[2].parse::<u32>().map_err(|_| INVALID)?;
        let (mut author, mut author_time, mut summary) = (None, None, None);
        let mut text = None;
        for record in records.by_ref() {
            if let Some(value) = record.strip_prefix('\t') {
                text = Some(value.to_owned());
                break;
            }
            if let Some(value) = record.strip_prefix("author ") {
                author = Some(value.to_owned());
            } else if let Some(value) = record.strip_prefix("author-time ") {
                author_time = Some(value.parse::<i64>().map_err(|_| INVALID)?);
            } else if let Some(value) = record.strip_prefix("summary ") {
                summary = Some(value.to_owned());
            }
        }
        if !result.commits.contains_key(&commit) {
            result.commits.insert(
                commit.clone(),
                BlameCommit {
                    author: author.ok_or(INVALID)?,
                    author_time: author_time.ok_or(INVALID)?,
                    summary: summary.ok_or(INVALID)?,
                },
            );
        }
        result.lines.push(BlameLine {
            line,
            commit,
            text: text.ok_or(INVALID)?,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORCELAIN: &str = concat!(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 1 1 2\n",
        "author Kim\nauthor-mail <kim@example.test>\nauthor-time 1790000000\nauthor-tz +0900\n",
        "committer Kim\ncommitter-mail <kim@example.test>\ncommitter-time 1790000000\ncommitter-tz +0900\n",
        "summary first commit\nfilename a.txt\n\tline one\n",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 2 2\n\tline two\n",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 3 3 1\n",
        "author Lee\nauthor-mail <lee@example.test>\nauthor-time 1790000500\nauthor-tz +0900\n",
        "committer Lee\ncommitter-mail <lee@example.test>\ncommitter-time 1790000500\ncommitter-tz +0900\n",
        "summary second\nprevious aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa a.txt\nfilename a.txt\n\tline three\n",
    );

    #[test]
    fn porcelain_blame_reuses_commit_headers() {
        let blame = parse_porcelain(PORCELAIN, 10_000).unwrap();
        assert_eq!(blame.lines.len(), 3);
        assert_eq!(
            (blame.lines[1].line, blame.lines[1].text.as_str()),
            (2, "line two")
        );
        assert_eq!(blame.lines[1].commit, blame.lines[0].commit);
        assert_eq!(blame.commits[&blame.lines[2].commit].author, "Lee");
        assert_eq!(
            blame.commits[&blame.lines[0].commit].summary,
            "first commit"
        );
        assert!(!blame.truncated);
    }

    #[test]
    fn line_limit_truncates() {
        let blame = parse_porcelain(PORCELAIN, 2).unwrap();
        assert_eq!(blame.lines.len(), 2);
        assert!(blame.truncated);
    }
}
