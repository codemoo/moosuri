use std::{fs::OpenOptions, io::Write, path::Path, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if let [
        command,
        source_flag,
        source,
        capsule_flag,
        capsule,
        uv_flag,
        uv,
    ] = args.as_slice()
        && command == "paired-prepare"
        && source_flag == "--source"
        && capsule_flag == "--capsule"
        && uv_flag == "--uv"
        && [source, capsule, uv]
            .iter()
            .all(|p| Path::new(p).is_absolute())
    {
        return match moosuri_eval::paired::prepare(
            Path::new(source),
            Path::new(capsule),
            Path::new(uv),
        )
        .await
        {
            Ok(()) => {
                println!("{{\"capsule_prepared\":true,\"subject_execution\":\"not_run\"}}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    let paired = match args.as_slice() {
        [command, flag, capsule, report, path]
            if matches!(command.as_str(), "paired-baseline" | "paired-check")
                && flag == "--capsule"
                && report == "--report"
                && Path::new(capsule).is_absolute()
                && Path::new(path).is_absolute() =>
        {
            Some(capsule)
        }
        _ => None,
    };
    let (source, report_path) = match args.as_slice() {
        [_, _, _, _, path] if paired.is_some() => (None, Some(path)),
        [] => (None, None),
        [flag, path] if flag == "--report" && Path::new(path).is_absolute() => (None, Some(path)),
        [command, flag, source]
            if command == "baseline-pin"
                && flag == "--source"
                && Path::new(source).is_absolute() =>
        {
            (Some(source), None)
        }
        [command, flag, source, report, path]
            if command == "baseline-pin"
                && flag == "--source"
                && Path::new(source).is_absolute()
                && report == "--report"
                && Path::new(path).is_absolute() =>
        {
            (Some(source), Some(path))
        }
        _ => {
            eprintln!(
                "Usage: moosuri-eval [baseline-pin --source /absolute/legacy-root] [--report /absolute/new-report.json]"
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
    let (success, mut bytes) = if let Some(capsule) = paired {
        if args[0] == "paired-check" {
            let report = moosuri_eval::paired::check(Path::new(capsule));
            (report.success, serde_json::to_vec_pretty(&report).unwrap())
        } else {
            let report = moosuri_eval::paired::run(Path::new(capsule)).await;
            (report.success, serde_json::to_vec_pretty(&report).unwrap())
        }
    } else if let Some(source) = source {
        let report = moosuri_eval::baseline::preflight(Path::new(source));
        (report.success, serde_json::to_vec_pretty(&report).unwrap())
    } else {
        let report = moosuri_eval::run().await;
        (report.success, serde_json::to_vec_pretty(&report).unwrap())
    };
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
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
