//! Pure project attribution of directories and Docker Compose containers, shared by the
//! observe and runtime domains. Nothing here touches the filesystem or logs its input.
use serde::{Deserialize, Deserializer, de::IgnoredAny};
use std::path::{Component, Path, PathBuf};

/// Composition input: a domain never reads the projects database for this.
#[derive(Debug, Clone)]
pub struct ProjectRoot {
    pub id: String,
    pub path: PathBuf,
}

const WORKING_DIR: &str = "com.docker.compose.project.working_dir=";
/// A longer label value is not a directory this machine can have (PATH_MAX).
const MAX_DIR_BYTES: usize = 4096;

/// The project whose root is `dir` or contains it; the longest root wins. Paths are compared
/// component by component and only as written: `..` is refused instead of resolved, because a
/// symlink on the way could make either answer wrong.
pub fn owning_root<'a>(dir: &Path, roots: &'a [ProjectRoot]) -> Option<&'a str> {
    if !dir.is_absolute() || dir.components().any(|part| part == Component::ParentDir) {
        return None;
    }
    roots
        .iter()
        .filter(|root| root.path.is_absolute() && dir.starts_with(&root.path))
        .max_by_key(|root| root.path.components().count())
        .map(|root| root.id.as_str())
}

/// The project a container belongs to, from the `Labels` column of `docker ps --format json`.
/// Labels are untrusted and decide only how a container is grouped on screen.
pub fn compose_project<'a>(labels: &str, roots: &'a [ProjectRoot]) -> Option<&'a str> {
    let dir = working_dir(labels)?;
    if dir.len() > MAX_DIR_BYTES {
        return None;
    }
    owning_root(Path::new(dir), roots)
}

/// Docker joins `key=value` labels with unescaped commas. A piece without `=` therefore
/// continues the value before it. A directory whose name has a comma followed later by `=`
/// cannot be told from two labels and is read up to that comma.
fn working_dir(labels: &str) -> Option<&str> {
    let mut value: Option<(usize, usize)> = None;
    let mut start = 0;
    for piece in labels.split(',') {
        let end = start + piece.len();
        match &mut value {
            Some((_, value_end)) if !piece.contains('=') => *value_end = end,
            Some(_) => break,
            None if piece.starts_with(WORKING_DIR) => {
                value = Some((start + WORKING_DIR.len(), end))
            }
            None => (),
        }
        start = end + 1;
    }
    value.map(|(from, to)| &labels[from..to])
}

/// `Labels` of a `docker ps --format json` row. Anything but a string counts as no labels, so
/// one odd row does not fail the listing.
pub fn docker_labels<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Labels {
        Text(String),
        Other(IgnoredAny),
    }
    Ok(match Labels::deserialize(deserializer)? {
        Labels::Text(text) => text,
        Labels::Other(_) => String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(paths: &[(&str, &str)]) -> Vec<ProjectRoot> {
        paths
            .iter()
            .map(|(id, path)| ProjectRoot {
                id: (*id).into(),
                path: (*path).into(),
            })
            .collect()
    }
    fn label(dir: &str) -> String {
        format!(
            "com.docker.compose.project=app,com.docker.compose.project.working_dir={dir},com.docker.compose.service=web"
        )
    }

    #[test]
    fn owning_root_matches_whole_components_and_the_longest_root() {
        let roots = roots(&[("app", "/p/app"), ("api", "/p/app/services/api")]);
        let owner = |dir: &str| owning_root(Path::new(dir), &roots);
        assert_eq!(owner("/p/app"), Some("app"), "the root itself");
        assert_eq!(owner("/p/app/web"), Some("app"), "inside the root");
        assert_eq!(owner("/p/app/"), Some("app"), "trailing slash");
        assert_eq!(owner("/p//app/./web"), Some("app"), "repeated separators");
        assert_eq!(owner("/p/app2"), None, "sibling sharing a string prefix");
        assert_eq!(owner("/p/app2/web"), None);
        assert_eq!(owner("/p"), None, "parent of the root");
        assert_eq!(
            owner("/p/app/services/api/db"),
            Some("api"),
            "nested projects: the longest root wins"
        );
        assert_eq!(owner("/p/app/services"), Some("app"));
    }

    #[test]
    fn owning_root_refuses_relative_empty_and_parent_paths() {
        let roots = roots(&[("app", "/p/app"), ("other", "/p/other"), ("empty", "")]);
        let owner = |dir: &str| owning_root(Path::new(dir), &roots);
        assert_eq!(owner(""), None);
        assert_eq!(owner("p/app"), None, "relative");
        assert_eq!(owner("./p/app"), None);
        assert_eq!(
            owner("/p/app/../other"),
            None,
            "`..` is refused, not resolved: a symlink could make either answer wrong"
        );
        assert_eq!(owner("/p/app/.."), None);
        assert_eq!(owner("/q/x"), None, "an empty root contains nothing");
    }

    #[test]
    fn compose_project_reads_the_working_dir_label() {
        let roots = roots(&[("app", "/home/u/projects/app")]);
        let project = |labels: &str| compose_project(labels, &roots);
        assert_eq!(project(&label("/home/u/projects/app")), Some("app"));
        assert_eq!(project(&label("/home/u/projects/app/deploy")), Some("app"));
        assert_eq!(project(&label("/home/u/projects/app/")), Some("app"));
        assert_eq!(project(&label("/home/u/projects/app2")), None);
        assert_eq!(project(&label("projects/app")), None, "relative label");
        assert_eq!(project(&label("")), None, "empty label");
        assert_eq!(project(""), None, "no labels");
        assert_eq!(
            project("com.docker.compose.project=app,com.docker.compose.service=web"),
            None,
            "a container without the label stays unattributed"
        );
        assert_eq!(
            project("x.com.docker.compose.project.working_dir=/home/u/projects/app"),
            None,
            "the key is matched whole"
        );
        assert_eq!(
            project("com.docker.compose.project.working_dir=/home/u/projects/app"),
            Some("app"),
            "last label"
        );
    }

    #[test]
    fn compose_project_bounds_the_label_and_rejoins_values_docker_split_at_commas() {
        let roots = roots(&[
            ("app", "/home/u/projects/app"),
            ("comma", "/home/u/projects/app,v2"),
        ]);
        let project = |labels: &str| compose_project(labels, &roots);
        let long = format!("/home/u/projects/app/{}", "d".repeat(MAX_DIR_BYTES));
        assert_eq!(project(&label(&long)), None, "over-long label");
        // Docker joins labels with unescaped commas: a value with a comma arrives in pieces.
        assert_eq!(
            project(&label("/home/u/projects/app,v2")),
            Some("comma"),
            "the pieces without `=` belong to the directory"
        );
        assert_eq!(project(&label("/home/u/projects/app,v3/web")), None);
        // Known limit: a comma followed by `=` reads as the next label.
        assert_eq!(project(&label("/home/u/projects/app,v2=x")), Some("app"));
        assert_eq!(
            project(
                "com.docker.compose.depends_on=db:service_started:false,cache:service_started:false,com.docker.compose.project.working_dir=/home/u/projects/app"
            ),
            Some("app"),
            "other labels with commas do not hide the next label"
        );
    }

    #[derive(Deserialize)]
    struct Row {
        #[serde(rename = "Labels", default, deserialize_with = "docker_labels")]
        labels: String,
    }
    #[test]
    fn docker_labels_accept_any_json_type() {
        let labels = |json: &str| {
            serde_json::from_str::<Row>(json)
                .expect("a row with odd labels still parses")
                .labels
        };
        assert_eq!(labels(r#"{"Labels":"a=b,c=d"}"#), "a=b,c=d");
        for odd in [
            r#"{}"#,
            r#"{"Labels":null}"#,
            r#"{"Labels":7}"#,
            r#"{"Labels":["a=b"]}"#,
            r#"{"Labels":{"a":"b"}}"#,
        ] {
            assert_eq!(labels(odd), "", "{odd}");
        }
    }
}
