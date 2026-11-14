use std::{fs::OpenOptions, io::Write, path::Path, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let report_path = match args.as_slice() {
        [] => None,
        [flag, path] if flag == "--report" && Path::new(path).is_absolute() => Some(path),
        _ => {
            eprintln!(
                "Usage: cargo run --locked -p moosuri-eval -- [--report /absolute/new-report.json]"
            );
            return ExitCode::from(2);
        }
    };
    // Reserve before executing; create_new rejects overwrite/symlink targets.
    let mut file = if let Some(path) = report_path {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(path) {
            Ok(file) => Some(file),
            Err(_) => {
                eprintln!("report_create_failed");
                return ExitCode::from(2);
            }
        }
    } else {
        None
    };
    let report = moosuri_eval::run().await;
    let mut bytes = serde_json::to_vec_pretty(&report).unwrap();
    bytes.push(b'\n');
    if bytes.len() > 128 * 1024 {
        eprintln!("report_size_exceeded");
        return ExitCode::from(2);
    }
    let written = if let Some(file) = &mut file {
        file.write_all(&bytes).and_then(|_| file.sync_all())
    } else {
        std::io::stdout().lock().write_all(&bytes)
    };
    if written.is_err() {
        eprintln!("report_write_failed");
        return ExitCode::from(2);
    }
    if report.success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
