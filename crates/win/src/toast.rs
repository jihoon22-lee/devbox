#[derive(Debug, Clone)]
pub struct ToastSpec {
    pub title: String,
    pub body: String,
    pub launch_uri: String,
    pub actions: Vec<(String, String)>,
}

fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            '\t'
            | '\n'
            | '\r'
            | '\u{20}'..='\u{d7ff}'
            | '\u{e000}'..='\u{fffd}'
            | '\u{10000}'..='\u{10ffff}' => escaped.push(character),
            _ => escaped.push('\u{fffd}'),
        }
    }
    escaped
}

pub fn xml(spec: &ToastSpec) -> String {
    let actions: String = spec
        .actions
        .iter()
        .map(|(label, uri)| {
            format!(
                r#"<action content="{}" activationType="protocol" arguments="{}"/>"#,
                escape(label),
                escape(uri)
            )
        })
        .collect();
    format!(
        r#"<toast activationType="protocol" launch="{}"><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual><actions>{}</actions></toast>"#,
        escape(&spec.launch_uri),
        escape(&spec.title),
        escape(&spec.body),
        actions
    )
}

pub fn tag_for(agent_id: &str) -> String {
    use sha2::{Digest, Sha256};
    if agent_id.encode_utf16().count() <= 58 {
        return format!("agent-{agent_id}");
    }
    // A distinct prefix prevents collisions with any short, literal ID. Use a
    // deterministic 232-bit digest instead of slicing UTF-8 or truncating IDs.
    let digest = Sha256::digest(agent_id.as_bytes());
    let mut tag = String::with_capacity(64);
    tag.push_str("agent#");
    for byte in &digest[..29] {
        use std::fmt::Write;
        let _ = write!(tag, "{byte:02x}");
    }
    tag
}

/// Run event tags use a separate namespace; later failures of a reused run remain distinct.
pub fn run_tag_for(event_id: &str) -> String {
    use sha2::{Digest, Sha256};
    if event_id.encode_utf16().count() <= 60 {
        return format!("run-{event_id}");
    }
    let digest = Sha256::digest(event_id.as_bytes());
    let mut tag = String::from("run#");
    for byte in &digest[..30] {
        use std::fmt::Write;
        let _ = write!(tag, "{byte:02x}");
    }
    tag
}

#[cfg(windows)]
pub fn show(aumid: &str, spec: &ToastSpec, tag: &str, group: &str) -> windows::core::Result<()> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::HSTRING;

    // Interfaces declared afterward are released before apartment teardown.
    let _apartment = crate::apartment::Apartment::enter()?;
    let document = XmlDocument::new()?;
    document.LoadXml(&HSTRING::from(xml(spec)))?;
    let toast = ToastNotification::CreateToastNotification(&document)?;
    toast.SetTag(&HSTRING::from(tag))?;
    toast.SetGroup(&HSTRING::from(group))?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(aumid))?.Show(&toast)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escapes_every_field_and_uses_protocol_activation() {
        let x = xml(&ToastSpec {
            title: "로그인 <수정> & 테스트".into(),
            body: "needs \"Bash\" 'now'".into(),
            launch_uri: "devbox://agents/a?x=1&y=\"2\"".into(),
            actions: vec![("<열기> & '확인'".into(), "devbox://agents/a?x=1&y=2".into())],
        });
        assert!(x.contains(
            r#"<toast activationType="protocol" launch="devbox://agents/a?x=1&amp;y=&quot;2&quot;""#
        ));
        assert!(x.contains("로그인 &lt;수정&gt; &amp; 테스트"));
        assert!(x.contains("needs &quot;Bash&quot; &apos;now&apos;"));
        assert!(x.contains(r#"<action content="&lt;열기&gt; &amp; &apos;확인&apos;" activationType="protocol" arguments="devbox://agents/a?x=1&amp;y=2"/>"#));
    }

    #[test]
    fn xml_replaces_forbidden_xml_characters_but_preserves_whitespace() {
        let x = xml(&ToastSpec {
            title: "a\0b\u{b}c\u{fffe}d\u{ffff}e\t\n\r😀".into(),
            body: String::new(),
            launch_uri: String::new(),
            actions: vec![],
        });
        assert!(x.contains("a�b�c�d�e\t\n\r😀"));
    }

    #[test]
    fn run_tags_are_distinct_from_agent_tags_and_utf16_bounded() {
        assert_eq!(run_tag_for("event-1"), "run-event-1");
        assert_ne!(run_tag_for("event-1"), tag_for("event-1"));
        for id in ["한글/".repeat(30), "😀".repeat(50)] {
            assert!(run_tag_for(&id).encode_utf16().count() <= 64);
            assert_eq!(run_tag_for(&id), run_tag_for(&id));
            assert_ne!(
                run_tag_for(&format!("{id}1")),
                run_tag_for(&format!("{id}2"))
            );
        }
        assert_eq!(
            run_tag_for(&"😀".repeat(30)),
            format!("run-{}", "😀".repeat(30))
        );
    }

    #[test]
    fn tags_are_stable_utf16_bounded_and_distinguish_long_ids() {
        assert_eq!(tag_for("a1b2"), "agent-a1b2");
        for id in ["x".repeat(100), "한".repeat(100), "😀".repeat(100)] {
            assert_eq!(tag_for(&id), tag_for(&id));
            assert!(tag_for(&id).encode_utf16().count() <= 64);
            assert_ne!(tag_for(&format!("{id}a")), tag_for(&format!("{id}b")));
        }
        let boundary = "😀".repeat(29);
        assert_eq!(tag_for(&boundary), format!("agent-{boundary}"));
        assert_ne!(
            tag_for(&format!("{boundary}a")),
            tag_for(&format!("{boundary}b"))
        );
    }
}
