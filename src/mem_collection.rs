use anyhow::{Result, anyhow};
use bollard::{Docker, query_parameters::StatsOptions};
use std::{collections::HashSet, ffi::OsStr, sync::LazyLock};

use bollard::config::ContainerStatsResponse;
use bollard::errors::Error;
use futures_util::{Stream, StreamExt};
use std::pin::Pin;

use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};

use crate::BackendArg;

pub enum Backends {
    ProcessBackend(ProcessBackend),
    DockerBackend(DockerBackend),
}

impl Backends {
    pub fn sample<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<u64>> + 'a>> {
        match self {
            Backends::ProcessBackend(process_backend) => Box::pin(process_backend.sample()),
            Backends::DockerBackend(docker_backend) => Box::pin(docker_backend.sample()),
        }
    }
}

pub trait Backend {
    fn sample(&mut self) -> impl std::future::Future<Output = Result<u64>>;
}

pub struct ProcessBackend {
    target_name: String,
    tracked: HashSet<Pid>,
    sys: System,
}

#[cfg(target_os = "linux")]
static REFRESH_KIND: LazyLock<ProcessRefreshKind> = LazyLock::new(|| ProcessRefreshKind::nothing());

#[cfg(not(target_os = "linux"))]
static REFRESH_KIND: LazyLock<ProcessRefreshKind> =
    LazyLock::new(|| ProcessRefreshKind::nothing().with_memory());

impl ProcessBackend {
    pub fn from_name(target_name: String) -> Result<Self> {
        let sys = System::new_with_specifics(RefreshKind::nothing().with_processes(*REFRESH_KIND));

        Ok(Self {
            tracked: find_pids(&sys, &target_name),
            target_name,
            sys,
        })
    }
}

impl Backend for ProcessBackend {
    async fn sample(&mut self) -> Result<u64> {
        if self.tracked.is_empty() {
            return Err(anyhow!("All processes finished"));
        }

        // On every step recheck whether new processes have been created
        self.sys
            .refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, *REFRESH_KIND);

        self.tracked.extend(find_pids(&self.sys, &self.target_name));

        let mut current_mem = 0;
        self.tracked
            .retain(|pid| match process_memory_kb(&self.sys, *pid) {
                Some(mem) => {
                    current_mem += mem;
                    true
                }
                None => false,
            });

        Ok(current_mem)
    }
}

fn find_pids(sys: &System, name: &str) -> HashSet<Pid> {
    sys.processes_by_name(OsStr::new(name))
        .map(|p| p.pid())
        .collect()
}

// On linux we parse smaps_rollup to get PSS, that is not to count shared memory
// multiple times, must return KB
#[cfg(target_os = "linux")]
fn process_memory_kb(_: &System, pid: Pid) -> Option<u64> {
    fn parse_pss(line: String) -> Option<u64> {
        line.strip_prefix("Pss:")
            .and_then(|s| s.trim_start().split_ascii_whitespace().next())
            .and_then(|s| s.parse().ok())
    }

    let path = format!("/proc/{}/smaps_rollup", pid);
    let file = std::fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);
    for line in std::io::BufRead::lines(reader) {
        let line = line.ok()?;
        if let Some(kb) = parse_pss(line) {
            return Some(kb);
        }
    }

    None
}

/// Returns KB
#[cfg(not(target_os = "linux"))]
fn process_memory_kb(system: &System, pid: Pid) -> Option<u64> {
    system.process(pid).map(|p| p.memory() / 1024)
}

type StatsStream = Pin<Box<dyn Stream<Item = Result<ContainerStatsResponse, Error>> + Send>>;

pub struct DockerBackend {
    stream: StatsStream,
}

impl DockerBackend {
    async fn connect(container: &str) -> Result<Self> {
        let docker = Docker::connect_with_socket_defaults()?;
        let stream = docker.stats(
            container,
            Some(StatsOptions {
                stream: true,
                ..Default::default()
            }),
        );

        Ok(Self {
            stream: Box::pin(stream),
        })
    }
}

impl Backend for DockerBackend {
    async fn sample(&mut self) -> Result<u64> {
        let stats = self
            .stream
            .next()
            .await
            .ok_or_else(|| anyhow!("Docker died"))??;

        stats
            .memory_stats
            .ok_or_else(|| anyhow!("Memory data not available"))?
            .usage
            .ok_or_else(|| anyhow!("Memory usage not available"))
            .map(|bytes| bytes / 1024)
    }
}

pub async fn create_backend(backend_arg: BackendArg, target: &str) -> Result<Backends> {
    match backend_arg {
        BackendArg::Docker => DockerBackend::connect(target)
            .await
            .map(|b| Backends::DockerBackend(b)),
        BackendArg::Process => {
            ProcessBackend::from_name(target.to_string()).map(|b| Backends::ProcessBackend(b))
        }
    }
}
