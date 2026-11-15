use moosuri_core::{ArtifactDraft, ArtifactExternalEvidence};
fn escape(v: &str) -> String {
    v.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn content(d: &ArtifactDraft, evidence: &[ArtifactExternalEvidence]) -> String {
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
    if !evidence.is_empty() {
        text.push_str("\n## Exact external read receipts\n");
        for e in evidence {
            text.push_str(&format!("- {} run {} step {} call {}\n  request: {}\n  request SHA256: {}\n  result SHA256: {}\n", e.kind, e.run_id, e.step, e.call_id, e.request, e.request_sha256, e.result_sha256));
        }
    }
    text
}
pub(super) fn markdown(d: &ArtifactDraft, evidence: &[ArtifactExternalEvidence]) -> String {
    format!("# {}\n\n{}", d.title, content(d, evidence))
}
/// Literal Markdown and typed record fields in a print-friendly pre. No HTML
/// parser, scripts, remote embeds, styles or clickable untrusted URLs.
pub(super) fn html(d: &ArtifactDraft, evidence: &[ArtifactExternalEvidence]) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; base-uri 'none'; form-action 'none'; style-src 'none'\"><title>{}</title></head><body><h1>{}</h1><pre>{}</pre></body></html>",
        escape(&d.title),
        escape(&d.title),
        escape(&content(d, evidence))
    )
}
