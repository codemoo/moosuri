//! Read-only public package inspection, with no installation or private account.
use moosuri_connectors::github_skills::{GitHubSkillSource, acquire_github_skill};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: inspect_public_skill <https://github.com/owner/repo> <directory> <ref-or-full-commit>".into());
    }
    let package = acquire_github_skill(GitHubSkillSource {
        repository: args[0].clone(),
        directory: args[1].clone(),
        reference: args[2].clone(),
    })
    .await?;
    let (source, commit, files) = package.into_parts();
    println!(
        "repository={} directory={} commit={} files={} decoded_bytes={}",
        source.repository(),
        source.directory(),
        commit,
        files.len(),
        files.values().map(String::len).sum::<usize>()
    );
    Ok(())
}
