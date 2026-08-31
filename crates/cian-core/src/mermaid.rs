//! Turning a document's ```mermaid fences into a page a browser can draw.
//!
//! Both front ends do this: the terminal build has no way to draw a diagram at
//! all and hands it to a browser, and the window draws them inline but still
//! wants the browser for one big enough to read. The extractor and the page
//! were written once, here, so `:mermaid` means the same thing in both.

/// Pull the contents of each ```mermaid ...``` fenced block out of `source`.
pub fn extract_blocks(source: &[String]) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < source.len() {
        let t = source[i].trim_start();
        let is_mermaid_fence = (t.starts_with("```") || t.starts_with("~~~"))
            && t.trim_start_matches(['`', '~']).trim().eq_ignore_ascii_case("mermaid");
        if is_mermaid_fence {
            i += 1;
            let mut body = String::new();
            while i < source.len()
                && !(source[i].trim_start().starts_with("```") || source[i].trim_start().starts_with("~~~"))
            {
                body.push_str(&source[i]);
                body.push('\n');
                i += 1;
            }
            i += 1; // consume the closing fence
            if !body.trim().is_empty() {
                blocks.push(body);
            }
        } else {
            i += 1;
        }
    }
    blocks
}

/// Wrap the blocks in a self-contained page. `script` is however mermaid is
/// being loaded — a local copy beside the page, or the CDN.
pub fn page(blocks: &[String], script: &str) -> String {
    let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut body = String::new();
    for b in blocks {
        body.push_str("<pre class=\"mermaid\">\n");
        body.push_str(&escape(b));
        body.push_str("</pre>\n");
    }
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>cian — mermaid</title>\
<style>body{{background:#0f1116;color:#cdd0d8;font-family:system-ui,sans-serif;margin:0;padding:20px}}\
h3{{margin:0 0 12px;font-weight:600}}\
.mermaid{{background:#fff;border-radius:10px;padding:16px;margin:16px 0;overflow:auto}}</style>\
</head><body><h3>cian — mermaid</h3>{body}{script}</body></html>"
    )
}
