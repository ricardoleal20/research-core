// The ChopFlow queue compute-target adapter (Story 6.4, FR-22.3, AD-6):
// a ChopFlow deployment as a queue-shaped compute target — FIRST-CLASS
// BUT OPTIONAL. An unavailable or unconfigured ChopFlow is an honest,
// typed error state on that target only; never a crash, and no other
// flow requires it (the other adapters never touch it).
//
// CLIENT CHOICE (documented — no ChopFlow SDK): the adapter speaks a
// MINIMAL HTTP/JSON slice of the ChopFlow queue API with the existing
// reqwest client (the provider layer's HTTP stack, AD-9's library):
//
//   POST {endpoint}/v0/jobs            {"queue"?, "spec": JobSpec}
//                                      → 200/201 {"id": "..."}
//   GET  {endpoint}/v0/jobs/{id}       → {"state": "queued"|"running"|
//                                         "finished"|"failed",
//                                         "reason"?, "code"?}
//   GET  {endpoint}/v0/jobs/{id}/artifacts
//                                      → {"code"?, "stdout", "stderr"}
//   GET  {endpoint}/health             → 200 (the probe)
//
// The spec crosses the wire VERBATIM as structured JSON — cmd, args,
// env, resources, workdir — never a shell string (AD-6; JobSpec
// validation re-runs at this boundary anyway). Dogfooding note: this is
// the same wire contract ChopFlow itself consumes, so a ResearchCore
// workspace can queue onto its own deployment unchanged.
//
// Each HTTP call is ONE BOUNDED round trip (10s) on a dedicated thread
// with a one-shot runtime — the ComputeTarget trait is synchronous, and
// reqwest's blocking client refuses async contexts. Connection pooling
// across calls is a documented v0.2.0 limit, honestly: one fresh
// connection per action.
//
// DOWN-STATES: a connect failure or timeout is the typed
// `endpoint_unreachable:` error — at submit nothing lands in the log
// (the user sees the typed error), at monitor the job keeps its last
// observed state honestly (the poll loop skips the target). A job the
// queue no longer knows is the typed `unknown_job:`. ChopFlow carries
// NO allowlist gate (the story asks for honest availability, not a
// gate) — its endpoint is validated at declaration (http/https, one
// token).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::domain::jobs::JobSpec;

use super::{ComputeTarget, JobHandle, JobResult, TargetError, TargetInfo, TargetJobStatus};

/// How long one HTTP round trip may take — a stuck queue endpoint is a
/// typed unreachable, never a hung job row.
const HTTP_TIMEOUT_SECS: u64 = 10;

/// The per-target config, parsed and re-validated at the adapter
/// boundary (defense in depth behind the `target.declared` validation).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ChopFlowConfig {
    /// The queue endpoint (config `endpoint`, an http/https URL).
    endpoint: String,
    /// The queue jobs are enqueued on (config `queue`, optional — the
    /// deployment's default queue when absent).
    queue: Option<String>,
}

fn parse_config(target: &TargetInfo) -> Result<ChopFlowConfig, TargetError> {
    let endpoint = target
        .cfg("endpoint")
        .ok_or_else(|| TargetError::MissingConfig {
            target: target.name.clone(),
            key: "endpoint".into(),
            expectation: "the ChopFlow queue endpoint the target submits to".into(),
        })?
        .to_string();
    if !(endpoint.starts_with("http://") || endpoint.starts_with("https://")) {
        return Err(TargetError::InvalidConfig {
            key: "endpoint".into(),
            reason: format!("`{endpoint}` — the endpoint is an http(s) URL"),
        });
    }
    Ok(ChopFlowConfig {
        endpoint,
        queue: target.cfg("queue").map(str::to_string),
    })
}

// ---------------------------------------------------------------------------
// The HTTP slice (one bounded round trip per action)
// ---------------------------------------------------------------------------

/// How one round trip can fail — mapped to typed TargetErrors outside.
enum HttpFail {
    /// Connect/timeout/transport — the honest down-state.
    Unreachable(String),
    /// The queue answered with a non-2xx (404 on a job GET becomes
    /// UnknownJob outside; the rest is the down-state with the queue's
    /// own words).
    Status(u16, String),
    /// A 2xx whose body does not parse — the endpoint is not speaking
    /// the documented slice.
    Bad(String),
}

/// Run one HTTP action on a dedicated thread with a one-shot runtime
/// (the trait is sync; reqwest's blocking client refuses async
/// contexts). `f` receives a fresh client and the endpoint's base URL.
fn round_trip<T, F, Fut>(endpoint: &str, f: F) -> Result<T, HttpFail>
where
    T: Send + 'static,
    F: FnOnce(reqwest::Client, reqwest::Url) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, HttpFail>> + Send + 'static,
{
    let url: reqwest::Url = endpoint
        .parse()
        .map_err(|e| HttpFail::Bad(format!("invalid endpoint URL: {e}")))?;
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| HttpFail::Unreachable(format!("runtime: {e}")))?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build()
            .map_err(|e| HttpFail::Unreachable(format!("client: {e}")))?;
        runtime.block_on(f(client, url))
    })
    .join()
    .map_err(|e| HttpFail::Unreachable(format!("http thread panicked: {e:?}")))?
}

/// Map a round trip's failure to the typed down-state (404 on a job
/// resource is the caller's UnknownJob — passed through by `not_found`).
fn typed_fail(config: &ChopFlowConfig, fail: HttpFail, not_found: bool) -> TargetError {
    match fail {
        HttpFail::Unreachable(detail) => TargetError::EndpointUnreachable {
            endpoint: config.endpoint.clone(),
            detail,
        },
        HttpFail::Status(404, detail) if not_found => {
            TargetError::UnknownJob(format!("{} ({detail})", config.endpoint))
        }
        HttpFail::Status(status, detail) => TargetError::EndpointUnreachable {
            endpoint: config.endpoint.clone(),
            detail: format!("the queue answered {status}: {detail}"),
        },
        HttpFail::Bad(detail) => TargetError::EndpointUnreachable {
            endpoint: config.endpoint.clone(),
            detail,
        },
    }
}

/// One JSON request/response round trip.
async fn json_call<T: serde::de::DeserializeOwned + Send + 'static>(
    client: reqwest::Client,
    url: reqwest::Url,
    method: reqwest::Method,
    body: Option<serde_json::Value>,
) -> Result<T, HttpFail> {
    let mut request = client.request(method, url);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request
        .send()
        .await
        .map_err(|e| HttpFail::Unreachable(e.to_string()))?;
    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(|e| HttpFail::Unreachable(e.to_string()))?;
    if !(200..300).contains(&status) {
        let detail = text.lines().next().unwrap_or("").chars().take(200).collect();
        return Err(HttpFail::Status(status, detail));
    }
    serde_json::from_str(&text).map_err(|e| HttpFail::Bad(format!("unparsable response ({e}): {}", text.chars().take(120).collect::<String>())))
}

// ---------------------------------------------------------------------------
// The queue's wire shapes
// ---------------------------------------------------------------------------

/// `POST /v0/jobs` → the queued job's id.
#[derive(Debug, serde::Deserialize)]
struct JobCreated {
    id: String,
}

/// `GET /v0/jobs/{id}` → the queue's state for the job.
#[derive(Debug, serde::Deserialize)]
struct JobState {
    state: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    code: Option<i32>,
}

/// `GET /v0/jobs/{id}/artifacts` → the finished job's captured output.
#[derive(Debug, serde::Deserialize, Default)]
struct Artifacts {
    #[serde(default)]
    code: Option<i32>,
    #[serde(default)]
    stdout: String,
    #[serde(default)]
    stderr: String,
}

/// What one queue-state read resolved to.
enum QueueState {
    Alive,
    Terminal { code: Option<i32>, reason: Option<String> },
}

// ---------------------------------------------------------------------------
// The adapter
// ---------------------------------------------------------------------------

/// A resolved terminal: exit 0 with no reason is Finished; everything
/// else is Failed with its reason.
#[derive(Debug, Clone)]
struct Terminal {
    code: Option<i32>,
    reason: Option<String>,
}

impl Terminal {
    fn status(&self) -> TargetJobStatus {
        match (self.code, self.reason.clone()) {
            (Some(0), None) => TargetJobStatus::Finished { code: 0 },
            (code, reason) => TargetJobStatus::Failed {
                reason: reason.unwrap_or_else(|| "unknown".into()),
                code,
            },
        }
    }
}

/// One live ChopFlow job: its config and its phase (the queue's id once
/// accepted, the terminal facts once resolved — cached: a terminal is
/// final). Shared by Arc; process-global like every adapter table.
struct Entry {
    config: ChopFlowConfig,
    phase: Mutex<Phase>,
}

#[derive(Debug)]
enum Phase {
    /// The queue accepted the job — its id, and the terminal facts once
    /// a read resolved them.
    Queued { id: String, terminal: Option<Terminal> },
}

/// Poison-tolerant lock (a panicking holder never cascades).
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn entries() -> &'static Mutex<HashMap<String, Arc<Entry>>> {
    static ENTRIES: OnceLock<Mutex<HashMap<String, Arc<Entry>>>> = OnceLock::new();
    ENTRIES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lookup_entry(handle: &JobHandle) -> Result<Arc<Entry>, TargetError> {
    lock(entries())
        .get(&handle.id)
        .cloned()
        .ok_or_else(|| TargetError::UnknownJob(handle.id.clone()))
}

/// The ChopFlow queue adapter (Story 6.4): structured specs enqueued as
/// JSON, queue states mapped onto the job lifecycle, artifacts fetched
/// as captured output.
pub struct ChopFlow;

impl ChopFlow {
    /// The adapter the registry registers (Story 3.2's seam).
    pub fn new() -> Self {
        ChopFlow
    }

    /// The reachability probe (the settings row's discovery state): a
    /// cheap `GET {endpoint}/health` — typed unreachable when the queue
    /// does not answer.
    pub fn probe(&self, target: &TargetInfo) -> Result<String, TargetError> {
        let config = parse_config(target)?;
        let endpoint = config.endpoint.clone();
        let health = round_trip(&endpoint, |client, base| async move {
            let url = base.join("health").map_err(|e| HttpFail::Bad(e.to_string()))?;
            let _: serde_json::Value =
                json_call(client, url, reqwest::Method::GET, None).await?;
            Ok(())
        })
        .map_err(|fail| typed_fail(&config, fail, false))?;
        let _ = health;
        Ok(format!("queue answered ({endpoint})"))
    }
}

impl Default for ChopFlow {
    fn default() -> Self {
        Self::new()
    }
}

impl ComputeTarget for ChopFlow {
    fn kind(&self) -> &'static str {
        "chopflow"
    }

    fn submit(&self, spec: &JobSpec, target: &TargetInfo) -> Result<JobHandle, TargetError> {
        // The adapter boundary validates again (AD-6): nothing is
        // enqueued that did not pass the never-freeform-shell gate.
        spec.validate()?;
        let config = parse_config(target)?;
        // One bounded round trip: enqueue the spec as structured JSON.
        // A down endpoint is a typed SUBMIT error — nothing lands in the
        // log, the user sees the honest down-state.
        let payload = serde_json::json!({
            "queue": config.queue,
            "spec": spec,
        });
        let endpoint = config.endpoint.clone();
        let created = round_trip(&endpoint, move |client, base| async move {
            let url = base.join("v0/jobs").map_err(|e| HttpFail::Bad(e.to_string()))?;
            json_call::<JobCreated>(client, url, reqwest::Method::POST, Some(payload)).await
        })
        .map_err(|fail| typed_fail(&config, fail, false))?;
        if created.id.trim().is_empty() {
            return Err(TargetError::EndpointUnreachable {
                endpoint: config.endpoint.clone(),
                detail: "the queue accepted the job but named no id".into(),
            });
        }
        let handle = JobHandle::new(uuid::Uuid::new_v4().to_string());
        lock(entries()).insert(handle.id.clone(), Arc::new(Entry {
            config,
            phase: Mutex::new(Phase::Queued { id: created.id, terminal: None }),
        }));
        Ok(handle)
    }

    fn monitor(&self, handle: &JobHandle) -> Result<TargetJobStatus, TargetError> {
        let entry = lookup_entry(handle)?;
        enum Due {
            Read(String),
        }
        let due = {
            let phase = lock(&entry.phase);
            match &*phase {
                Phase::Queued { id, terminal } => match terminal {
                    Some(terminal) => return Ok(terminal.status()),
                    None => Due::Read(id.clone()),
                },
            }
        };
        let Due::Read(id) = due;
        // One bounded round trip outside the lock.
        let endpoint = entry.config.endpoint.clone();
        let job_path = format!("v0/jobs/{id}");
        let state = round_trip(&endpoint, move |client, base| async move {
            let url = base.join(&job_path).map_err(|e| HttpFail::Bad(e.to_string()))?;
            json_call::<JobState>(client, url, reqwest::Method::GET, None).await
        })
        .map_err(|fail| typed_fail(&entry.config, fail, true))?;
        let resolved = match state.state.as_str() {
            "queued" | "running" => QueueState::Alive,
            "finished" => QueueState::Terminal { code: Some(0), reason: None },
            "failed" => QueueState::Terminal {
                code: state.code,
                reason: Some(state.reason.unwrap_or_else(|| "queue_failed".into())),
            },
            other => QueueState::Terminal {
                code: state.code,
                reason: Some(format!("queue_state_{other}")),
            },
        };
        let mut phase = lock(&entry.phase);
        let Phase::Queued { terminal, .. } = &mut *phase else {
            return Ok(TargetJobStatus::Running);
        };
        match resolved {
            QueueState::Alive => Ok(TargetJobStatus::Running),
            QueueState::Terminal { code, reason } => {
                let terminal_value = Terminal { code, reason };
                let status = terminal_value.status();
                *terminal = Some(terminal_value);
                Ok(status)
            }
        }
    }

    fn fetch(&self, handle: &JobHandle) -> Result<JobResult, TargetError> {
        let entry = lookup_entry(handle)?;
        let (id, terminal) = {
            let phase = lock(&entry.phase);
            let Phase::Queued { id, terminal } = &*phase else {
                return Err(TargetError::UnknownJob(handle.id.clone()));
            };
            let Some(terminal) = terminal else {
                return Err(TargetError::NotTerminal(handle.id.clone()));
            };
            (id.clone(), terminal.clone())
        };
        // One bounded round trip outside the lock.
        let endpoint = entry.config.endpoint.clone();
        let artifacts_path = format!("v0/jobs/{id}/artifacts");
        let artifacts = round_trip(&endpoint, move |client, base| async move {
            let url = base
                .join(&artifacts_path)
                .map_err(|e| HttpFail::Bad(e.to_string()))?;
            json_call::<Artifacts>(client, url, reqwest::Method::GET, None).await
        })
        .map_err(|fail| typed_fail(&entry.config, fail, true))?;
        Ok(JobResult {
            // the terminal's resolved code wins; the artifacts' code is
            // the fallback when the queue did not carry one in the state
            code: terminal.code.or(artifacts.code),
            stdout: artifacts.stdout,
            stderr: artifacts.stderr,
        })
    }
}

/// The fake ChopFlow deployment (test harness): a hand-rolled HTTP/1.1
/// server thread speaking the documented slice — shared by this
/// module's tests and the command-layer tests.
#[cfg(test)]
pub(crate) mod test_harness {
    use std::io::{Read, Write as _};
    use std::sync::{Arc, Mutex};

    /// One fake deployment: its endpoint URL, the state GET
    /// /v0/jobs/cf-1 answers (rewritable between polls), the captured
    /// POST body, and the artifacts body.
    pub(crate) struct FakeQueue {
        pub endpoint: String,
        pub state: Arc<Mutex<String>>,
        pub submitted: Arc<Mutex<Option<String>>>,
        pub artifacts: Arc<Mutex<String>>,
    }

    /// Read one HTTP request off the stream: (method, path, body).
    fn read_request(stream: &mut std::net::TcpStream) -> (String, String, String) {
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 4096];
        let header_end = loop {
            let n = stream.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                return (String::new(), String::new(), String::new());
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos;
            }
        };
        let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
        let mut lines = head.lines();
        let request_line = lines.next().unwrap_or_default().to_string();
        let content_length: usize = lines
            .filter_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse().ok())?
            })
            .next()
            .unwrap_or(0);
        while buf.len() < header_end + 4 + content_length {
            let n = stream.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let body = String::from_utf8_lossy(&buf[header_end + 4..]).into_owned();
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();
        (method, path, body)
    }

    /// Spawn the fake deployment on a random loopback port.
    pub(crate) fn spawn_fake_queue() -> FakeQueue {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let state: Arc<Mutex<String>> = Arc::new(Mutex::new(r#"{"state":"queued"}"#.into()));
        let submitted = Arc::new(Mutex::new(None));
        let artifacts: Arc<Mutex<String>> = Arc::new(Mutex::new(
            r#"{"code":0,"stdout":"queue out\n","stderr":"queue err"}"#.into(),
        ));
        let (state_t, submitted_t, artifacts_t) =
            (Arc::clone(&state), Arc::clone(&submitted), Arc::clone(&artifacts));
        std::thread::spawn(move || {
            for stream in listener.incoming().take(128) {
                let Ok(mut stream) = stream else { break };
                let (method, path, body) = read_request(&mut stream);
                let (code, reason, response) = match (method.as_str(), path.as_str()) {
                    ("POST", "/v0/jobs") => {
                        *submitted_t.lock().unwrap() = Some(body);
                        (201, "Created", r#"{"id":"cf-1"}"#.to_string())
                    }
                    ("GET", "/v0/jobs/cf-1") => {
                        (200, "OK", state_t.lock().unwrap().clone())
                    }
                    ("GET", "/v0/jobs/cf-1/artifacts") => {
                        (200, "OK", artifacts_t.lock().unwrap().clone())
                    }
                    ("GET", "/health") => (200, "OK", r#"{"ok":true}"#.to_string()),
                    _ => (404, "Not Found", r#"{"error":"not found"}"#.to_string()),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                    response.len()
                );
                let _ = stream.flush();
            }
        });
        FakeQueue { endpoint, state, submitted, artifacts }
    }
}

#[cfg(test)]
mod tests {
    use super::test_harness::spawn_fake_queue;
    use super::*;
    use crate::adapters::targets::TargetInfo;
    use std::collections::BTreeMap;

    fn spec(cmd: &str, args: &[&str]) -> JobSpec {
        JobSpec {
            cmd: cmd.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: BTreeMap::new(),
            resources: None,
            workdir: None,
        }
    }

    fn info(config: &[(&str, &str)]) -> TargetInfo {
        TargetInfo {
            name: "queue-1".into(),
            host: None,
            allowlist: Vec::new(),
            config: config
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    // ---- the gates: freeform, config ----

    #[tokio::test]
    async fn a_freeform_spec_is_refused_before_any_enqueue() {
        let queue = spawn_fake_queue();
        let chop = ChopFlow::new();
        let err = chop
            .submit(
                &spec("ls | rm -rf .", &[]),
                &info(&[("endpoint", &queue.endpoint)]),
            )
            .unwrap_err();
        assert!(err.to_string().starts_with("freeform_shell:"), "unexpected: {err}");
        // a missing or malformed endpoint is a typed refusal
        let err = chop.submit(&spec("python3", &[]), &info(&[])).unwrap_err();
        assert!(err.to_string().starts_with("missing_config:"), "unexpected: {err}");
        let err = chop
            .submit(
                &spec("python3", &[]),
                &info(&[("endpoint", "ftp://nope")]),
            )
            .unwrap_err();
        assert!(err.to_string().starts_with("invalid_config:"), "unexpected: {err}");
    }

    // ---- the lifecycle over the fake deployment ----

    #[tokio::test]
    async fn a_chopflow_job_lives_its_lifecycle_with_the_spec_enqueued_verbatim() {
        let queue = spawn_fake_queue();
        let chop = ChopFlow::new();
        let mut s = spec("python3", &["train.py", "--note=a | b && c"]);
        s.env.insert("EPOCHS".into(), "3".into());
        s.resources = Some(crate::domain::jobs::JobResources {
            cpus: Some(2),
            memory_mb: None,
        });
        let handle = chop
            .submit(&s, &info(&[("endpoint", &queue.endpoint), ("queue", "gpu-queue")]))
            .unwrap();
        // queued → running → finished, each an honest read
        assert_eq!(chop.monitor(&handle).unwrap(), TargetJobStatus::Running);
        *queue.state.lock().unwrap() = r#"{"state":"running"}"#.into();
        assert_eq!(chop.monitor(&handle).unwrap(), TargetJobStatus::Running);
        // fetch refuses while alive — typed, never a partial read
        let err = chop.fetch(&handle).unwrap_err();
        assert!(err.to_string().starts_with("job_not_terminal:"), "unexpected: {err}");
        // the spec crossed the wire VERBATIM — structured JSON, the
        // queue name riding along, never a shell string
        let submitted = queue.submitted.lock().unwrap().clone().unwrap();
        let sent: serde_json::Value = serde_json::from_str(&submitted).unwrap();
        assert_eq!(sent["queue"], serde_json::json!("gpu-queue"));
        assert_eq!(sent["spec"]["cmd"], serde_json::json!("python3"));
        assert_eq!(
            sent["spec"]["args"],
            serde_json::json!(["train.py", "--note=a | b && c"])
        );
        assert_eq!(sent["spec"]["env"]["EPOCHS"], serde_json::json!("3"));
        // finished: the artifacts are the captured output
        *queue.state.lock().unwrap() = r#"{"state":"finished"}"#.into();
        assert_eq!(
            chop.monitor(&handle).unwrap(),
            TargetJobStatus::Finished { code: 0 }
        );
        let result = chop.fetch(&handle).unwrap();
        assert_eq!(result.code, Some(0));
        assert_eq!(result.stdout.trim(), "queue out");
        assert_eq!(result.stderr.trim(), "queue err");
        // the terminal is cached — a second monitor agrees without rereading
        *queue.state.lock().unwrap() = r#"{"state":"running"}"#.into();
        assert_eq!(
            chop.monitor(&handle).unwrap(),
            TargetJobStatus::Finished { code: 0 }
        );
    }

    #[tokio::test]
    async fn a_failed_queue_job_is_a_reasoned_terminal() {
        let queue = spawn_fake_queue();
        *queue.state.lock().unwrap() =
            r#"{"state":"failed","reason":"container exited 3","code":3}"#.into();
        let chop = ChopFlow::new();
        let handle = chop
            .submit(&spec("python3", &[]), &info(&[("endpoint", &queue.endpoint)]))
            .unwrap();
        assert_eq!(
            chop.monitor(&handle).unwrap(),
            TargetJobStatus::Failed {
                reason: "container exited 3".into(),
                code: Some(3)
            }
        );
        // the artifacts still fetch (the captured output of the failure)
        let result = chop.fetch(&handle).unwrap();
        assert_eq!(result.code, Some(3));
    }

    // ---- the honest down-states (the story's core promise) ----

    #[tokio::test]
    async fn an_unreachable_queue_is_the_typed_down_state_and_never_a_crash() {
        // a port with nothing listening — the connection is refused
        let endpoint = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
            drop(listener);
            endpoint
        };
        let chop = ChopFlow::new();
        // the submit is a typed error — nothing lands anywhere
        let err = chop
            .submit(&spec("python3", &[]), &info(&[("endpoint", &endpoint)]))
            .unwrap_err();
        assert!(
            err.to_string().starts_with("endpoint_unreachable:"),
            "unexpected: {err}"
        );
        // the probe agrees — the settings row's discovery state
        let err = chop.probe(&info(&[("endpoint", &endpoint)])).unwrap_err();
        assert!(err.to_string().starts_with("endpoint_unreachable:"), "unexpected: {err}");
        // and a monitor on an accepted job keeps the last observed state
        // when the queue goes down mid-flight (typed, the poll skips)
        let queue = spawn_fake_queue();
        let handle = chop
            .submit(&spec("python3", &[]), &info(&[("endpoint", &queue.endpoint)]))
            .unwrap();
        assert_eq!(chop.monitor(&handle).unwrap(), TargetJobStatus::Running);
        // swap the stored entry to the dead port — the queue went away
        {
            let mut map = lock(entries());
            let (id, terminal, queue) = {
                let entry = map.get(&handle.id).unwrap();
                let Phase::Queued { id, terminal } = &*lock(&entry.phase) else {
                    panic!("expected the queued phase");
                };
                (id.clone(), terminal.clone(), entry.config.queue.clone())
            };
            map.insert(
                handle.id.clone(),
                Arc::new(Entry {
                    config: ChopFlowConfig { endpoint: endpoint.clone(), queue },
                    phase: Mutex::new(Phase::Queued { id, terminal }),
                }),
            );
        }
        let err = chop.monitor(&handle).unwrap_err();
        assert!(err.to_string().starts_with("endpoint_unreachable:"), "unexpected: {err}");
    }

    #[tokio::test]
    async fn a_job_the_queue_no_longer_knows_is_a_typed_unknown() {
        let queue = spawn_fake_queue();
        *queue.state.lock().unwrap() = String::new(); // unused; 404 comes from the path
        let chop = ChopFlow::new();
        let handle = chop
            .submit(&spec("python3", &[]), &info(&[("endpoint", &queue.endpoint)]))
            .unwrap();
        // rewrite the entry's queue id to one the fake does not know
        {
            let map = lock(entries());
            let entry = map.get(&handle.id).unwrap();
            *lock(&entry.phase) = Phase::Queued { id: "gone-job".into(), terminal: None };
        }
        let err = chop.monitor(&handle).unwrap_err();
        assert!(err.to_string().starts_with("unknown_job:"), "unexpected: {err}");
    }

    #[tokio::test]
    async fn a_healthy_probe_answers_and_an_unknown_handle_is_typed() {
        let queue = spawn_fake_queue();
        let chop = ChopFlow::new();
        let ok = chop.probe(&info(&[("endpoint", &queue.endpoint)])).unwrap();
        assert!(ok.contains("queue answered"), "{ok}");
        let err = chop.monitor(&JobHandle::new("never-submitted")).unwrap_err();
        assert!(err.to_string().starts_with("unknown_job:"), "unexpected: {err}");
        let err = chop.fetch(&JobHandle::new("never-submitted")).unwrap_err();
        assert!(err.to_string().starts_with("unknown_job:"), "unexpected: {err}");
    }
}
