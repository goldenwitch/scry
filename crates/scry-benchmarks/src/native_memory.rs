//! Diagnostic process-memory sampling for benchmark workloads.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System};

/// The resident process-memory quantity available on the current platform.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResidentMemoryKind {
    /// Windows working-set memory.
    #[cfg_attr(not(windows), allow(dead_code))]
    WorkingSet,
    /// Unix resident-set memory.
    #[cfg_attr(windows, allow(dead_code))]
    ResidentSet,
}

/// The secondary process-memory quantity available on the current platform.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SecondaryMemoryKind {
    /// Windows private usage, exposed by sysinfo from `PrivateUsage`.
    #[cfg_attr(not(windows), allow(dead_code))]
    Private,
    /// The platform's virtual-memory quantity when private usage is unavailable.
    #[cfg_attr(windows, allow(dead_code))]
    Virtual,
}

/// One process-memory observation with platform-specific semantics preserved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProcessMemory {
    pub(crate) resident_bytes: u64,
    pub(crate) resident_kind: ResidentMemoryKind,
    pub(crate) secondary_bytes: u64,
    pub(crate) secondary_kind: SecondaryMemoryKind,
}

/// The largest observed samples collected by one sampler run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct MaximumObservedMemory {
    pub(crate) samples: u64,
    pub(crate) resident_bytes: u64,
    pub(crate) resident_kind: Option<ResidentMemoryKind>,
    pub(crate) secondary_bytes: u64,
    pub(crate) secondary_kind: Option<SecondaryMemoryKind>,
}

impl MaximumObservedMemory {
    fn observe(&mut self, sample: ProcessMemory) -> Result<(), String> {
        self.samples = self
            .samples
            .checked_add(1)
            .ok_or_else(|| "native memory sample count overflowed".to_owned())?;
        if let Some(kind) = self.resident_kind
            && kind != sample.resident_kind
        {
            return Err("native resident memory metric changed during sampling".to_owned());
        }
        if let Some(kind) = self.secondary_kind
            && kind != sample.secondary_kind
        {
            return Err("native memory metric changed during sampling".to_owned());
        }
        self.resident_kind = Some(sample.resident_kind);
        self.secondary_kind = Some(sample.secondary_kind);
        self.resident_bytes = self.resident_bytes.max(sample.resident_bytes);
        self.secondary_bytes = self.secondary_bytes.max(sample.secondary_bytes);
        Ok(())
    }
}

/// Samples the current process until the returned sampler is finished.
pub(crate) struct Sampler {
    stop: Arc<AtomicBool>,
    observed: Arc<Mutex<MaximumObservedMemory>>,
    join: Option<JoinHandle<Result<(), String>>>,
}

impl Sampler {
    /// Starts sampling at `interval` until [`Self::finish`] is called.
    pub(crate) fn start(interval: Duration) -> Result<Self, String> {
        if interval.is_zero() {
            return Err("native memory sample interval must be positive".to_owned());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let observed = Arc::new(Mutex::new(MaximumObservedMemory::default()));
        let thread_stop = Arc::clone(&stop);
        let thread_observed = Arc::clone(&observed);
        let join = thread::Builder::new()
            .name("scry-native-memory".to_owned())
            .spawn(move || sample_loop(&thread_stop, &thread_observed, interval))
            .map_err(|error| format!("could not start native memory sampler: {error}"))?;
        Ok(Self {
            stop,
            observed,
            join: Some(join),
        })
    }

    /// Stops sampling and returns the maximum observed samples collected so far.
    pub(crate) fn finish(mut self) -> Result<MaximumObservedMemory, String> {
        self.stop.store(true, Ordering::Relaxed);
        let Some(join) = self.join.take() else {
            return Err("native memory sampler was already finished".to_owned());
        };
        match join.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err("native memory sampler thread panicked".to_owned()),
        }
        let observed = self
            .observed
            .lock()
            .map_err(|_| "native memory sampler state was poisoned".to_owned())?;
        if observed.samples == 0 {
            return Err("native memory sampler collected no samples".to_owned());
        }
        Ok(*observed)
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn sample_loop(
    stop: &Arc<AtomicBool>,
    observed: &Arc<Mutex<MaximumObservedMemory>>,
    interval: Duration,
) -> Result<(), String> {
    let pid = Pid::from_u32(std::process::id());
    let mut system = System::new();
    while !stop.load(Ordering::Relaxed) {
        let sample = sample_process(&mut system, pid)?;
        observed
            .lock()
            .map_err(|_| "native memory sampler state was poisoned".to_owned())?
            .observe(sample)?;
        thread::sleep(interval);
    }
    Ok(())
}

fn sample_process(system: &mut System, pid: Pid) -> Result<ProcessMemory, String> {
    let pids = [pid];
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        false,
        ProcessRefreshKind::nothing().with_memory(),
    );
    let process = system
        .process(pid)
        .ok_or_else(|| format!("process {pid} was not visible to the native memory sampler"))?;
    Ok(process_memory(process))
}

fn process_memory(process: &Process) -> ProcessMemory {
    #[cfg(windows)]
    {
        ProcessMemory {
            resident_bytes: process.memory(),
            resident_kind: ResidentMemoryKind::WorkingSet,
            secondary_bytes: process.virtual_memory(),
            secondary_kind: SecondaryMemoryKind::Private,
        }
    }
    #[cfg(not(windows))]
    {
        ProcessMemory {
            resident_bytes: process.memory(),
            resident_kind: ResidentMemoryKind::ResidentSet,
            secondary_bytes: process.virtual_memory(),
            secondary_kind: SecondaryMemoryKind::Virtual,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        MaximumObservedMemory, ProcessMemory, ResidentMemoryKind, Sampler, SecondaryMemoryKind,
    };

    #[test]
    fn a_zero_interval_is_rejected() {
        assert!(Sampler::start(Duration::ZERO).is_err());
    }

    #[test]
    fn a_maximum_observed_keeps_the_largest_sample() {
        let mut observed = MaximumObservedMemory::default();
        assert!(
            observed
                .observe(ProcessMemory {
                    resident_bytes: 10,
                    resident_kind: ResidentMemoryKind::ResidentSet,
                    secondary_bytes: 20,
                    secondary_kind: SecondaryMemoryKind::Private,
                })
                .is_ok()
        );
        assert!(
            observed
                .observe(ProcessMemory {
                    resident_bytes: 30,
                    resident_kind: ResidentMemoryKind::ResidentSet,
                    secondary_bytes: 15,
                    secondary_kind: SecondaryMemoryKind::Private,
                })
                .is_ok()
        );
        assert_eq!(observed.samples, 2);
        assert_eq!(observed.resident_bytes, 30);
        assert_eq!(
            observed.resident_kind,
            Some(ResidentMemoryKind::ResidentSet)
        );
        assert_eq!(observed.secondary_bytes, 20);
    }

    #[test]
    fn a_metric_kind_change_is_rejected() {
        let mut observed = MaximumObservedMemory::default();
        assert!(
            observed
                .observe(ProcessMemory {
                    resident_bytes: 10,
                    resident_kind: ResidentMemoryKind::ResidentSet,
                    secondary_bytes: 20,
                    secondary_kind: SecondaryMemoryKind::Private,
                })
                .is_ok()
        );
        assert!(
            observed
                .observe(ProcessMemory {
                    resident_bytes: 30,
                    resident_kind: ResidentMemoryKind::ResidentSet,
                    secondary_bytes: 40,
                    secondary_kind: SecondaryMemoryKind::Virtual,
                })
                .is_err()
        );
    }

    #[test]
    fn a_resident_metric_kind_change_is_rejected() {
        let mut observed = MaximumObservedMemory::default();
        assert!(
            observed
                .observe(ProcessMemory {
                    resident_bytes: 10,
                    resident_kind: ResidentMemoryKind::ResidentSet,
                    secondary_bytes: 20,
                    secondary_kind: SecondaryMemoryKind::Private,
                })
                .is_ok()
        );
        assert!(
            observed
                .observe(ProcessMemory {
                    resident_bytes: 30,
                    resident_kind: ResidentMemoryKind::WorkingSet,
                    secondary_bytes: 40,
                    secondary_kind: SecondaryMemoryKind::Private,
                })
                .is_err()
        );
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn the_current_process_has_a_memory_observation() {
        let result = super::sample_process(
            &mut sysinfo::System::new(),
            sysinfo::Pid::from_u32(std::process::id()),
        );
        assert!(result.is_ok_and(|sample| {
            if sample.resident_bytes == 0 {
                return false;
            }
            #[cfg(windows)]
            {
                sample.resident_kind == ResidentMemoryKind::WorkingSet
            }
            #[cfg(not(windows))]
            {
                sample.resident_kind == ResidentMemoryKind::ResidentSet
            }
        }));
    }
}
