use aperip_nomos::markdown::render_license_markdown;

#[test]
fn renders_license_markdown_and_normalizes_document_headings() {
    let markdown = "# License\n\n## Terms\n\n1. Keep this notice.\n\n`LICENSE`\n\n```text\n[A]  exact spacing\n```";

    let rendered = render_license_markdown(markdown);

    assert!(rendered.contains("<h2>License</h2>"));
    assert!(rendered.contains("<h3>Terms</h3>"));
    assert!(rendered.contains("<ol>"));
    assert!(rendered.contains("<code>LICENSE</code>"));
    assert!(
        rendered.contains("<pre><code class=\"language-text\">[A]  exact spacing\n</code></pre>")
    );
}

#[test]
fn removes_active_content_and_unsafe_resource_urls() {
    let markdown = r#"<script>alert(1)</script>

[unsafe](javascript:alert(1))

![tracking](https://tracker.example/pixel.png)

[official](https://ahcl.aperip.com)
"#;

    let rendered = render_license_markdown(markdown);

    assert!(!rendered.contains("<script"));
    assert!(!rendered.contains("javascript:"));
    assert!(!rendered.contains("<img"));
    assert!(!rendered.contains("tracker.example"));
    assert!(rendered.contains("href=\"https://ahcl.aperip.com\""));
    assert!(rendered.contains("rel=\"noopener noreferrer\""));
}

#[test]
fn rejects_relative_and_non_web_link_protocols() {
    let markdown = "[relative](/admin) [network](//evil.example) [data](data:text/html,x) [file](file:///etc/passwd) [mail](mailto:legal@example.com)";

    let rendered = render_license_markdown(markdown);

    assert!(!rendered.contains("href=\"/admin\""));
    assert!(!rendered.contains("evil.example"));
    assert!(!rendered.contains("data:text"));
    assert!(!rendered.contains("file:///"));
    assert!(rendered.contains("href=\"mailto:legal@example.com\""));
}

#[test]
fn preserves_raw_markdown_as_escaped_text_instead_of_executing_it() {
    let rendered = render_license_markdown("Before <b onclick=\"alert(1)\">bold</b> after");

    assert!(!rendered.contains("<b"));
    assert!(rendered.contains("&lt;b onclick="));
    assert!(rendered.contains("Before"));
    assert!(rendered.contains("bold"));
    assert!(rendered.contains("after"));
}
