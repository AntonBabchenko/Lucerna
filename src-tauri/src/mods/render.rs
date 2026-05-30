//! Render mod/modpack descriptions to sanitized HTML for the detail
//! modals. Modrinth bodies are CommonMark markdown; CurseForge bodies
//! are HTML. Both pass through `ammonia` — the single trust boundary for
//! externally-authored HTML reaching the webview. `ammonia`'s default
//! allowlist strips `<script>`/`<iframe>`, event-handler attributes,
//! inline `style`, and `javascript:`/`data:` URLs, and forces
//! `rel="noopener noreferrer"` on links. Image scheme is additionally
//! constrained by the app CSP (`img-src 'self' data: https:`).

/// Sanitize raw HTML (CurseForge description, or already-rendered
/// markdown) against the ammonia allowlist. We start from ammonia's
/// default (which strips scripts/iframes/event-handlers/inline-styles and
/// rewrites unsafe URL schemes) and additionally remove form controls:
/// a mod description has no legitimate use for `<form>`/`<input>`, and an
/// allowed `<form action=...>` could POST from the user's machine.
pub fn sanitize_html(raw: &str) -> String {
    ammonia::Builder::default()
        .rm_tags(["form", "input", "button", "select", "textarea"])
        .clean(raw)
        .to_string()
}

/// True for image URLs safe to place in an `<img src>` in the webview.
/// Platform gallery CDNs always serve https; restricting to it keeps the
/// gallery off `data:` / `http:` / exotic schemes — defence in depth
/// alongside the app CSP (`img-src 'self' data: https:`).
pub fn is_safe_image_url(url: &str) -> bool {
    url.starts_with("https://")
}

/// Convert CommonMark markdown (Modrinth body) to sanitized HTML.
pub fn markdown_to_safe_html(md: &str) -> String {
    use pulldown_cmark::{html, Options, Parser};
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(md, opts);
    let mut rendered = String::new();
    html::push_html(&mut rendered, parser);
    sanitize_html(&rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_renders_headings_and_links() {
        let html = markdown_to_safe_html("# Title\n\nSee [site](https://example.com).");
        assert!(html.contains("<h1>"));
        assert!(html.contains("href=\"https://example.com\""));
    }

    #[test]
    fn sanitizer_strips_script_tags() {
        let html = sanitize_html("<p>ok</p><script>alert(1)</script>");
        assert!(html.contains("<p>ok</p>"));
        assert!(!html.contains("<script"));
        assert!(!html.contains("alert(1)"));
    }

    #[test]
    fn sanitizer_drops_javascript_href() {
        let html = sanitize_html(r#"<a href="javascript:alert(1)">x</a>"#);
        assert!(!html.contains("javascript:"));
    }

    #[test]
    fn sanitizer_strips_event_handlers() {
        let html = sanitize_html(r#"<img src="https://x/y.png" onerror="alert(1)">"#);
        assert!(html.contains("src=\"https://x/y.png\""));
        assert!(!html.contains("onerror"));
    }

    #[test]
    fn sanitizer_keeps_https_images() {
        let html = markdown_to_safe_html("![alt](https://media.forgecdn.net/a.png)");
        assert!(html.contains("<img"));
        assert!(html.contains("https://media.forgecdn.net/a.png"));
    }

    #[test]
    fn sanitizer_strips_forms() {
        let html =
            sanitize_html(r#"<form action="https://evil/x"><input name="y"></form><p>ok</p>"#);
        assert!(!html.contains("<form"));
        assert!(!html.contains("<input"));
        assert!(!html.contains("action="));
        assert!(html.contains("<p>ok</p>"));
    }

    #[test]
    fn image_url_scheme_guard() {
        assert!(is_safe_image_url("https://media.forgecdn.net/a.png"));
        assert!(!is_safe_image_url("http://insecure/a.png"));
        assert!(!is_safe_image_url(
            "data:text/html,<script>alert(1)</script>"
        ));
        assert!(!is_safe_image_url("javascript:alert(1)"));
    }
}
