use crate::fixture::*;
use crate::render::{Budget, ImageResult, render, render_bounded};
use crate::*;
use data_encoding::BASE64;

fn png(size: usize) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x02\x08\x06\0\0\0".to_vec();
    bytes.resize(size, 0);
    bytes
}

#[test]
fn render_strips_scripts_event_handlers_and_remote_images() {
    let body = "<script>alert(1)</script>\n\n<p onclick=\"steal()\" onmouseover=\"x()\">hi</p>\n\n\
                <iframe src=\"https://e/\"></iframe>\n\n![r](https://e/x.png)\n\n<img src=\"http://e/y.png\" onerror=\"z()\">";
    let (html, _) = render(body, &|_| ImageResult::NotFound);
    for banned in [
        "<script",
        "alert(",
        "onclick",
        "onmouseover",
        "onerror",
        "<iframe",
        "src=\"http",
    ] {
        assert!(!html.contains(banned), "{banned} in {html}");
    }
    assert!(html.contains("hi"));
}

#[test]
fn remote_images_are_blocked_with_a_notice() {
    let loaded = std::cell::Cell::new(0);
    let loader = |_: &str| {
        loaded.set(loaded.get() + 1);
        ImageResult::Inlined("data:image/png;base64,AAAA".into())
    };
    let (html, _) = render(
        "![x](https://e/x.png) ![y](HTTP://e/y.png) ![z](//e/z.png)",
        &loader,
    );
    assert_eq!(loaded.get(), 0, "remote images never reach the loader");
    assert_eq!(html.matches("<img").count(), 0);
    assert_eq!(html.matches(r#"data-reason="remote-blocked""#).count(), 3);
    let (html, _) = render(r#"<img src="http://e/y.png" alt="raw">"#, &loader);
    assert_eq!(html.matches(r#"src="http"#).count(), 0);
    assert!(
        html.contains("<img"),
        "only the src attribute is dropped: {html}"
    );
    // Links keep http(s) hrefs.
    let (html, _) = render("[l](https://e/page)", &loader);
    assert!(html.contains(r#"href="https://e/page""#));
}

#[test]
fn image_budget_replaces_extra_images_with_budget_placeholders() {
    // 2,000,000-byte PNGs: 2,666,668 base64 bytes each, so three fit in 8 MiB.
    let uri = format!("data:image/png;base64,{}", BASE64.encode(&png(2_000_000)));
    let body = (0..5)
        .map(|i| format!("![i{i}](i{i}.png)\n\n"))
        .collect::<String>();
    let (html, _) = render_bounded(
        &body,
        &|_| ImageResult::Inlined(uri.clone()),
        Budget::default(),
    )
    .expect("fits the response budget");
    assert_eq!(html.matches("src=\"data:image/png").count(), 3);
    assert_eq!(html.matches(r#"data-reason="budget""#).count(), 2);
    let first_budget = html.find(r#"data-reason="budget""#).expect("notice");
    assert!(
        html.rfind("src=\"data:").expect("image") < first_budget,
        "later images go first"
    );
}

#[test]
fn mermaid_sources_beyond_the_response_budget_are_emptied_from_the_end() {
    let body = format!(
        "```mermaid\nA\n```\n\n```mermaid\n{}\n```\n",
        "B".repeat(200)
    );
    let body = body.as_str();
    let small = Budget {
        images: 0,
        total: 200,
    };
    let (html, mermaid) =
        render_bounded(body, &|_| ImageResult::NotFound, small).expect("html fits");
    assert_eq!(mermaid, ["A\n", ""]);
    assert!(
        html.contains(r#"data-idx="1" data-reason="budget""#),
        "{html}"
    );
    let tiny = Budget {
        images: 0,
        total: 10,
    };
    assert!(render_bounded(body, &|_| ImageResult::NotFound, tiny).is_err());
}

#[tokio::test]
async fn note_images_inline_only_from_the_vault() -> T {
    let n = fixture(&[("d/note.md", "x\n")]).await;
    n.write("d/pic.png", png(64));
    n.write("big.png", png(2 * 1024 * 1024 + 1));
    std::fs::write(n.home.join("secret.png"), png(64))?;
    std::os::unix::fs::symlink(n.home.join("secret.png"), n.vault().join("d/link.png"))?;
    let text = "---\ntitle: t\n---\n![a](pic.png) ![b](../big.png) ![c](missing.png) \
                ![d](link.png) ![e](../../secret.png) ![f](https://e/x.png)";
    let out = n
        .svc
        .render(RenderNote {
            path: Some("d/note.md".into()),
            text: Some(text.into()),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(out.html.matches("src=\"data:image/png;base64,").count(), 1);
    for reason in ["too-large", "not-found", "outside-root", "remote-blocked"] {
        assert!(
            out.html.contains(&format!(r#"data-reason="{reason}""#)),
            "{reason}"
        );
    }
    assert_eq!(out.html.matches(r#"data-reason="outside-root""#).count(), 2);
    assert!(
        !out.html.contains("title: t"),
        "frontmatter is not rendered"
    );
    Ok(())
}

#[tokio::test]
async fn note_render_marks_images_past_the_budget_and_the_count_limit() -> T {
    let n = fixture(&[("note.md", "x\n")]).await;
    for i in 0..5 {
        n.write(&format!("big{i}.png"), png(2_000_000));
    }
    let text: String = (0..5).map(|i| format!("![b{i}](big{i}.png)\n\n")).collect();
    let render = |text: String| {
        n.svc.render(RenderNote {
            path: Some("note.md".into()),
            text: Some(text),
        })
    };
    let out = render(text).await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(out.html.matches("src=\"data:image/png").count(), 3);
    assert_eq!(out.html.matches(r#"data-reason="budget""#).count(), 2);
    assert!(!out.html.contains("not-found"), "{}", &out.html[..200]);
    // More distinct images than one response loads: the rest are budget notices too.
    let count = crate::ops::MAX_IMAGES + 2;
    for i in 0..count {
        n.write(&format!("s/{i}.png"), png(64));
    }
    let text: String = (0..count).map(|i| format!("![s](s/{i}.png)\n")).collect();
    let out = render(text).await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        out.html.matches("src=\"data:image/png").count(),
        crate::ops::MAX_IMAGES
    );
    assert_eq!(out.html.matches(r#"data-reason="budget""#).count(), 2);
    Ok(())
}

#[tokio::test]
async fn images_resolving_into_another_root_are_not_inlined() -> T {
    let n = fixture(&[("d/note.md", "x\n")]).await;
    std::fs::write(n.home.join("project/x.png"), png(64))?;
    std::os::unix::fs::symlink(n.home.join("project/x.png"), n.vault().join("d/proj.png"))?;
    let out = n
        .svc
        .render(RenderNote {
            path: Some("d/note.md".into()),
            text: Some("![p](proj.png)".into()),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(!out.html.contains("src=\"data:"), "{}", out.html);
    assert!(out.html.contains(r#"data-reason="outside-root""#));
    Ok(())
}
