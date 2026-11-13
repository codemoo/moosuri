use moosuri_core::ArtifactDraft;
fn escape(v: &str) -> String {
    v.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn content(d: &ArtifactDraft) -> String {
    let fields =
        serde_json::to_string_pretty(&d.payload).expect("closed typed artifact payload serializes");
    let mut text = format!(
        "{}\n\n## Record fields\n\n```json\n{}\n```\n",
        d.body, fields
    );
    if !d.references.is_empty() {
        text.push_str("\n## Saved references (bounded display)\n");
        for r in &d.references {
            text.push_str(&format!(
                "- {:?}: {} revision {}\n",
                r.kind, r.id, r.revision
            ));
        }
    }
    text
}
pub(super) fn markdown(d: &ArtifactDraft) -> String {
    format!("# {}\n\n{}", d.title, content(d))
}
/// Literal Markdown and typed record fields in a print-friendly pre. No HTML
/// parser, scripts, remote embeds, styles or clickable untrusted URLs.
pub(super) fn html(d: &ArtifactDraft) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; base-uri 'none'; form-action 'none'; style-src 'none'\"><title>{}</title></head><body><h1>{}</h1><pre>{}</pre></body></html>",
        escape(&d.title),
        escape(&d.title),
        escape(&content(d))
    )
}
