use std::time::Duration;

use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use crate::mem_collection::create_backend;

use tokio::signal::unix::{SignalKind, signal};

use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};

mod mem_collection;

#[derive(Serialize)]
struct Output {
    memory_kb: u64,
}

#[derive(Parser, Debug)]
#[command(name = "memof", about = "Monitor process memory")]
struct Args {
    #[command(subcommand)]
    command: Action,
}

#[derive(Clone, Debug, Default, Deserialize, ValueEnum, Serialize)]
enum BackendArg {
    Process,

    #[default]
    Docker,
}

#[derive(Parser, Debug)]
enum Action {
    Start {
        name: String,

        #[arg(value_enum, default_value_t = BackendArg::Docker)]
        backend: BackendArg,
    },
    Stop,
}

#[derive(Serialize, Deserialize)]
struct State {
    backend: BackendArg,
    target: String,
}

const TIME_BETWEEN_PASSES: u64 = 200;
const MEMOF_PID_FILE: &str = "/tmp/memof.pid";

async fn run_monitor(state: State) {
    let mut backend = create_backend(state.backend, &state.target)
        .await
        .expect("Failed to load backend");

    let mut max_mem: u64 = 0;
    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to set SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("Failed to set SIGINT handler");

    loop {
        tokio::select! {
            _ = sigterm.recv() => break,
            _ = sigint.recv() => break,
            result = backend.sample() => {
                match result {
                    Ok(current_mem) => max_mem = std::cmp::max(current_mem, max_mem),
                    Err(_) => break,
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(TIME_BETWEEN_PASSES)).await;
    }

    let out = sonic_rs::to_string_pretty(&Output { memory_kb: max_mem })
        .expect("Failed to transform to json");

    std::fs::write("memory.json", out).expect("Failed to write to output file");
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if std::env::var("MEMOF_BACKGROUND").is_ok() {
        let state_str = std::env::var("MEMOF_STATE").expect("MEMOF_STATE not set");
        let state: State = sonic_rs::from_str(&state_str).expect("Failed to read MEMOF_STATE data");
        run_monitor(state).await;
        return;
    }

    match Args::parse().command {
        Action::Start { name, backend } => {
            let state = State {
                backend,
                target: name,
            };

            let child = std::process::Command::new(
                std::env::current_exe().expect("Failed to get current exe"),
            )
            .env("MEMOF_BACKGROUND", "1")
            .env(
                "MEMOF_STATE",
                sonic_rs::to_string(&state).expect("Failed to write MEMOF_STATE var"),
            )
            .spawn()
            .unwrap();

            std::fs::write(MEMOF_PID_FILE, child.id().to_string())
                .expect("Failed to write background pid");

            println!("Started monitor PID {}", child.id());
        }
        Action::Stop => {
            let pid: i32 = std::fs::read_to_string(MEMOF_PID_FILE)
                .expect("Failed to read background pid, are you sure it is running?")
                .parse()
                .expect("Pid is not well written");

            let _ = signal::kill(Pid::from_raw(pid), Signal::SIGTERM);
            if !wait_for_exit(pid, Duration::from_secs(5)).await {
                let _ = signal::kill(Pid::from_raw(pid), Signal::SIGKILL);
            }

            if let Err(e) = std::fs::remove_file(MEMOF_PID_FILE) {
                eprintln!("Failed to remove pid file {MEMOF_PID_FILE}: {e:?}");
            }
        }
    }
}

async fn wait_for_exit(pid: i32, timeout: Duration) -> bool {
    let mut sys = System::new();
    let pid = sysinfo::Pid::from_u32(pid as u32);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing(),
        );

        if sys.process(pid).is_none() {
            return true;
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    false
}
