use std::path::PathBuf;
use std::time::Duration;

use kael::process_model::{ProcessClass, ProcessInfo};
use kael::runtime::worker_client::WorkerClient;
use kael::runtime::worker_host::WorkerHost;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexFilesTask {
    directory: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileCounts {
    total_files: usize,
    total_dirs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexProgress {
    scanned: usize,
    current_path: String,
}

fn worker_main() {
    let client = match WorkerClient::connect_from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Worker failed to connect: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = client.run(|request, on_progress| match request {
        kael::process_model::WorkerRequest::Execute { payload } => {
            let task: IndexFilesTask = match serde_json::from_value(payload) {
                Ok(t) => t,
                Err(e) => {
                    return Err(kael::process_model::WorkerError::Execution(e.to_string()));
                }
            };

            let mut total_files = 0usize;
            let mut total_dirs = 0usize;
            let mut scanned = 0usize;

            let result = walk_directory(
                &task.directory,
                &mut total_files,
                &mut total_dirs,
                &mut scanned,
                &on_progress,
            );
            match result {
                Ok(_) => Ok(kael::process_model::WorkerResponse::Result(
                    serde_json::to_value(FileCounts {
                        total_files,
                        total_dirs,
                    })
                    .unwrap(),
                )),
                Err(e) => Err(kael::process_model::WorkerError::Execution(e)),
            }
        }
        kael::process_model::WorkerRequest::Ping => Ok(kael::process_model::WorkerResponse::Pong),
    }) {
        eprintln!("Worker error: {}", e);
        std::process::exit(1);
    }
}

fn walk_directory(
    dir: &PathBuf,
    total_files: &mut usize,
    total_dirs: &mut usize,
    scanned: &mut usize,
    on_progress: &(dyn Fn(kael::process_model::WorkerProgress) + Send),
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;

    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        *scanned += 1;

        if (*scanned).is_multiple_of(10) {
            on_progress(kael::process_model::WorkerProgress::Update(
                serde_json::to_value(IndexProgress {
                    scanned: *scanned,
                    current_path: path.to_string_lossy().to_string(),
                })
                .unwrap(),
            ));
        }

        if path.is_dir() {
            *total_dirs += 1;
            walk_directory(&path, total_files, total_dirs, scanned, on_progress)?;
        } else {
            *total_files += 1;
        }
    }

    Ok(())
}

fn host_main() {
    let exe = std::env::current_exe().expect("current exe");
    let mut host = WorkerHost::with_temp_dir();

    let info = ProcessInfo::new(
        kael::process_model::ProcessId(0),
        ProcessClass::Worker,
        "file-indexer",
    )
    .executable(&exe)
    .arg("--worker");

    println!("Spawning worker process...");
    let handle = host
        .spawn_worker(ProcessClass::Worker, info)
        .expect("spawn worker");

    let task = IndexFilesTask {
        directory: std::env::current_dir().expect("current dir"),
    };

    println!("Sending indexing task for: {:?}", task.directory);

    let rx = handle.stream_progress(task).expect("stream progress");

    let mut last_scanned = 0usize;
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(progress)) => {
                let prog: IndexProgress = progress;
                if prog.scanned > last_scanned {
                    println!(
                        "Progress: scanned {} files, currently at {}",
                        prog.scanned, prog.current_path
                    );
                    last_scanned = prog.scanned;
                }
            }
            Ok(Err(e)) => {
                eprintln!("Worker error: {:?}", e);
                break;
            }
            Err(_) => {
                println!("No more progress updates.");
                break;
            }
        }
    }

    let task = IndexFilesTask {
        directory: std::env::current_dir().expect("current dir"),
    };
    match handle.request::<_, FileCounts>(task) {
        Ok(counts) => {
            println!(
                "Indexing complete: {} files, {} directories",
                counts.total_files, counts.total_dirs
            );
        }
        Err(e) => {
            eprintln!("Request failed: {}", e);
        }
    }

    println!("Demonstrating fire-and-forget ping...");
    let _ = handle.fire_and_forget(()).ok();

    println!("Daemon app finished.");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--worker") {
        worker_main();
    } else {
        host_main();
    }
}
