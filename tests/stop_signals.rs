//! A scheduled run stops on the signals a container runtime, a service manager and a closed terminal
//! send as it stops on Ctrl+C: its current cycle ends at a stopping point, it reports that the
//! schedule finished, it leaves nothing in the temporary folder, and it exits with status 0.

#![cfg(unix)]

use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use pretty_assertions::assert_eq;
use std::{
    fs,
    io::{BufRead, BufReader},
    mem,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};
use temp_dir::TempDir;

/// How long the run may take to reach a line or to end, far beyond what a run that handles the
/// signal needs, so a run that ignores it fails the test instead of hanging it.
const GRACE: Duration = Duration::from_secs(30);

/// How often the test checks whether the run has ended.
const POLL: Duration = Duration::from_millis(20);

/// The CSV document the run reads: two stations.
const STATIONS: &str = "id,name,temperature\nS1,Centrum,12.5\nS2,Haven,11.0\n";

/// A mapping of the stations into `Station` entities.
const MAPPING: &str = r#"{
    version: "v4",
    dataModel: "Station",
    identity: { entityName: "{{ id }}" },
    attributes: {
        name: { source: "{{ name }}" },
        temperature: { source: "{{ temperature }}", transformation: "float" },
    },
}"#;

/// A manifest that maps the stations into files once a minute, with no bound, so only a stop ends
/// it.
const MANIFEST: &str = r#"{
    version: "v1",
    schedule: { mode: "every", value: "1m" },
    inputs: [{ source: "stations.csv", mapping: "station.json5" }],
    output: { target: "file", directory: "out" },
}"#;

/// A `cassiopeia map` run in a process of its own, whose stderr lines a thread forwards as they
/// come, and which is killed when the test ends before it does.
struct Run {
    child: Child,
    lines: Receiver<String>,
    seen: Vec<String>,
}

impl Run {
    /// Starts `cassiopeia map --manifest run.json5` in `folder`, with an empty configuration file and
    /// every folder the run keeps files in under `folder`, the temporary folder `tmp` among them, so
    /// nothing outside it shapes the run.
    fn start(folder: &Path) -> Run {
        let config = folder.join("config.toml");
        fs::write(&config, "").unwrap();
        fs::create_dir(folder.join("tmp")).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_cassiopeia"))
            .current_dir(folder)
            .arg("--config")
            .arg(&config)
            .args(["map", "--manifest", "run.json5"])
            .env("CASSIOPEIA_SCHEMAS__FOLDER", folder.join("schemas"))
            .env("CASSIOPEIA_MAPPINGS__FOLDER", folder.join("mappings"))
            .env("CASSIOPEIA_CURSORS__FOLDER", folder.join("cursors"))
            .env("TMPDIR", folder.join("tmp"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = child.stderr.take().unwrap();
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else {
                    break;
                };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Run {
            child,
            lines,
            seen: Vec::new(),
        }
    }

    /// Waits until the run has written a line holding `text`.
    fn wait_for(&mut self, text: &str) {
        let deadline = Instant::now() + GRACE;
        while !self.seen.iter().any(|line| line.contains(text)) {
            match self.lines.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(line) => self.seen.push(line),
                Err(error) => panic!("the run never wrote `{text}` ({error}): {:?}", self.seen),
            }
        }
    }

    /// Sends `signal` to the run.
    fn send(&self, signal: Signal) {
        kill(Pid::from_raw(i32::try_from(self.child.id()).unwrap()), signal).unwrap();
    }

    /// Waits for the run to end and returns its exit status and every line it wrote.
    fn end(mut self) -> (ExitStatus, Vec<String>) {
        let deadline = Instant::now() + GRACE;
        let status = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "the run did not end: {:?}", self.seen);
            thread::sleep(POLL);
        };
        // The forwarding thread ends once the ended run's stderr is closed.
        self.seen.extend(self.lines.iter());
        (status, mem::take(&mut self.seen))
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        // A run left behind by a failed assertion would go on running its schedule. Killing a run
        // that has ended and been waited for does nothing.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts the scheduled run, sends it `signal` once its first cycle has completed, and asserts that
/// it ends as a stopped schedule ends, its temporary folder as empty as it found it.
fn assert_signal_stops_the_schedule(signal: Signal) {
    let folder = TempDir::new().unwrap();
    fs::write(folder.path().join("stations.csv"), STATIONS).unwrap();
    fs::write(folder.path().join("station.json5"), MAPPING).unwrap();
    fs::write(folder.path().join("run.json5"), MANIFEST).unwrap();
    let mut run = Run::start(folder.path());

    run.wait_for("#00001 completed");
    run.send(signal);
    let (status, lines) = run.end();

    assert_eq!(status.code(), Some(0), "{lines:?}");
    assert!(lines.last().is_some_and(|line| line.contains("Schedule finished")), "{lines:?}");
    assert!(!lines.iter().any(|line| line.contains("#00002")), "{lines:?}");
    let left: Vec<PathBuf> = fs::read_dir(folder.path().join("tmp")).unwrap().map(|entry| entry.unwrap().path()).collect();
    assert_eq!(left, Vec::<PathBuf>::new());
}

#[test]
fn sigint_stops_a_scheduled_run_between_two_cycles() {
    assert_signal_stops_the_schedule(Signal::SIGINT);
}

#[test]
fn sigterm_stops_a_scheduled_run_as_ctrl_c_does() {
    assert_signal_stops_the_schedule(Signal::SIGTERM);
}

#[test]
fn sighup_stops_a_scheduled_run_as_ctrl_c_does() {
    assert_signal_stops_the_schedule(Signal::SIGHUP);
}
