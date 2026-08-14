/// semver 파서와 비교. `v` 접두사는 여기서 다루지 않는다 (release 계층에서만 정규화).
/// prerelease는 같은 major.minor.patch의 stable보다 낮게 정렬된다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    prerelease: Option<String>,
}

impl Version {
    pub fn parse(s: &str) -> Result<Version, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("빈 버전 문자열".into());
        }
        if s.starts_with('v') {
            return Err(format!(
                "버전에 v 접두사가 있다: {s} (release 계층에서 정규화)"
            ));
        }
        let (core, prerelease) = match s.split_once('-') {
            Some((core, pre)) => (core, Some(pre.to_string())),
            None => (s, None),
        };
        let mut parts = core.split('.');
        let major = parse_num(parts.next(), "major", s)?;
        let minor = parse_num(parts.next(), "minor", s)?;
        let patch = parse_num(parts.next(), "patch", s)?;
        if parts.next().is_some() {
            return Err(format!("버전 세그먼트가 너무 많다: {s}"));
        }
        Ok(Version {
            major,
            minor,
            patch,
            prerelease,
        })
    }

    pub fn is_prerelease(&self) -> bool {
        self.prerelease.is_some()
    }
}

fn parse_num(raw: Option<&str>, part: &str, full: &str) -> Result<u32, String> {
    let raw = raw.ok_or_else(|| format!("버전에 {part}이 없다: {full}"))?;
    raw.parse::<u32>()
        .map_err(|_| format!("버전의 {part}이 숫자가 아니다: {full}"))
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.prerelease, &other.prerelease) {
                (None, None) => std::cmp::Ordering::Equal,
                // stable > prerelease
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.prerelease {
            write!(f, "-{pre}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic() {
        let v = Version::parse("0.2.2").unwrap();
        assert_eq!((v.major, v.minor, v.patch), (0, 2, 2));
        assert!(!v.is_prerelease());
    }

    #[test]
    fn parses_prerelease() {
        let v = Version::parse("0.3.0-rc1").unwrap();
        assert_eq!((v.major, v.minor, v.patch), (0, 3, 0));
        assert!(v.is_prerelease());
    }

    #[test]
    fn rejects_v_prefix() {
        assert!(Version::parse("v0.2.0").is_err());
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Version::parse("").is_err());
        assert!(Version::parse("0.2").is_err());
        assert!(Version::parse("a.b.c").is_err());
        assert!(Version::parse("0.2.0.1").is_err());
    }

    #[test]
    fn orders_patch_versions() {
        assert!(Version::parse("0.2.0").unwrap() < Version::parse("0.2.2").unwrap());
        assert!(Version::parse("0.2.2").unwrap() < Version::parse("0.3.0").unwrap());
    }

    #[test]
    fn prerelease_sorts_below_stable() {
        assert!(Version::parse("0.3.0-rc1").unwrap() < Version::parse("0.3.0").unwrap());
    }

    #[test]
    fn equal_versions_compare_equal() {
        assert_eq!(
            Version::parse("1.2.3").unwrap(),
            Version::parse("1.2.3").unwrap()
        );
    }
}
