//! Device-independent conditional workflows and their execution engine.
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use async_trait::async_trait;
use miko_core::{Cancel, MikoError, MikoResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    pub name: String,
    pub nodes: Vec<FlowNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Locator {
    Ui {
        text: Option<String>,
        resource_id: Option<String>,
    },
    Template {
        path: String,
        threshold: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum FlowAction {
    Tap {
        point: Point,
    },
    TapElement {
        locator: Locator,
    },
    Swipe {
        from: Point,
        to: Point,
        duration_ms: u64,
    },
    Text {
        text: String,
    },
    Key {
        key: String,
    },
    Script {
        id: Uuid,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum FlowNode {
    Action {
        action: FlowAction,
    },
    Wait {
        ms: u64,
    },
    WaitElement {
        locator: Locator,
        present: bool,
        timeout_ms: u64,
        interval_ms: u64,
    },
    Branch {
        locator: Locator,
        then_nodes: Vec<FlowNode>,
        else_nodes: Vec<FlowNode>,
    },
    Retry {
        nodes: Vec<FlowNode>,
        max_attempts: u32,
        timeout_ms: u64,
        interval_ms: u64,
    },
}

impl Flow {
    pub fn validate(&self) -> MikoResult<()> {
        if self.name.trim().is_empty() {
            return Err(MikoError::usage("flow name is empty"));
        }
        validate_nodes(&self.nodes, 0)
    }
}

fn validate_locator(locator: &Locator) -> MikoResult<()> {
    match locator {
        Locator::Ui { text, resource_id }
            if text.as_ref().is_some_and(|s| !s.is_empty())
                || resource_id.as_ref().is_some_and(|s| !s.is_empty()) =>
        {
            Ok(())
        }
        Locator::Template { path, threshold }
            if !path.is_empty() && threshold.is_finite() && (0.0..=1.0).contains(threshold) =>
        {
            Ok(())
        }
        _ => Err(MikoError::usage("invalid element locator")),
    }
}

fn validate_point(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite() && (0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y)
}

fn validate_nodes(nodes: &[FlowNode], depth: usize) -> MikoResult<()> {
    if depth > 10 {
        return Err(MikoError::usage("flow nesting exceeds 10"));
    }
    for node in nodes {
        match node {
            FlowNode::Action { action } => match action {
                FlowAction::Tap { point } if !validate_point(*point) => {
                    return Err(MikoError::usage("invalid tap coordinates"));
                }
                FlowAction::Swipe {
                    from,
                    to,
                    duration_ms,
                } if !validate_point(*from)
                    || !validate_point(*to)
                    || *duration_ms == 0
                    || *duration_ms > crate::MAX_SCRIPT_MS =>
                {
                    return Err(MikoError::usage("invalid swipe"));
                }
                FlowAction::TapElement { locator } => validate_locator(locator)?,
                FlowAction::Key { key } if key.is_empty() => {
                    return Err(MikoError::usage("empty key"));
                }
                _ => {}
            },
            FlowNode::Wait { ms } if *ms > crate::MAX_SCRIPT_MS => {
                return Err(MikoError::usage("wait exceeds 24 hours"));
            }
            FlowNode::Wait { .. } => {}
            FlowNode::WaitElement {
                locator,
                timeout_ms,
                interval_ms,
                ..
            } => {
                validate_locator(locator)?;
                if *timeout_ms == 0
                    || *timeout_ms > crate::MAX_SCRIPT_MS
                    || *interval_ms == 0
                    || *interval_ms > crate::MAX_SCRIPT_MS
                {
                    return Err(MikoError::usage(
                        "wait requires a timeout and polling interval",
                    ));
                }
            }
            FlowNode::Branch {
                locator,
                then_nodes,
                else_nodes,
            } => {
                validate_locator(locator)?;
                validate_nodes(then_nodes, depth + 1)?;
                validate_nodes(else_nodes, depth + 1)?;
            }
            FlowNode::Retry {
                nodes,
                max_attempts,
                timeout_ms,
                interval_ms,
            } => {
                if *max_attempts == 0
                    || *max_attempts > crate::MAX_COUNT
                    || *timeout_ms == 0
                    || *timeout_ms > crate::MAX_SCRIPT_MS
                    || *interval_ms == 0
                    || *interval_ms > crate::MAX_SCRIPT_MS
                {
                    return Err(MikoError::usage(
                        "retry requires attempts, timeout and interval",
                    ));
                }
                validate_nodes(nodes, depth + 1)?;
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowLog {
    pub node: String,
    pub elapsed_ms: u64,
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowReport {
    pub name: String,
    pub elapsed_ms: u64,
    pub logs: Vec<FlowLog>,
    pub failed_node: Option<String>,
    pub error: Option<String>,
}

/// Implementations own device I/O and must finish releasing input before returning.
#[async_trait]
pub trait FlowHost: Send {
    /// Return a normalized center. Multiple matches must be reported as an error.
    async fn find(&mut self, locator: &Locator) -> MikoResult<Option<Point>>;
    async fn action(&mut self, action: &FlowAction, cancel: &Cancel) -> MikoResult<()>;
    /// Called before each node; a UI implementation may pause or single-step here.
    async fn checkpoint(&mut self, node: &str, cancel: &Cancel) -> MikoResult<()>;
}

struct Runner<'a, H> {
    host: &'a mut H,
    cancel: &'a Cancel,
    logs: Vec<FlowLog>,
    failed_node: Option<String>,
}

impl<H: FlowHost> Runner<'_, H> {
    fn check(&self, deadline: Option<Instant>) -> MikoResult<()> {
        self.cancel.check()?;
        if deadline.is_some_and(|end| Instant::now() >= end) {
            return Err(MikoError::unexpected("retry timed out"));
        }
        Ok(())
    }

    async fn sleep(&mut self, duration: Duration, deadline: Option<Instant>) -> MikoResult<()> {
        let end = Instant::now() + duration;
        while Instant::now() < end {
            self.check(deadline)?;
            let next = end.min(Instant::now() + Duration::from_millis(50));
            tokio::time::sleep_until(deadline.map_or(next, |d| d.min(next))).await;
        }
        self.check(deadline)
    }

    async fn find(
        &mut self,
        locator: &Locator,
        deadline: Option<Instant>,
    ) -> MikoResult<Option<Point>> {
        self.check(deadline)?;
        let task = self.host.find(locator);
        tokio::pin!(task);
        loop {
            tokio::select! {
                result = &mut task => return result,
                () = tokio::time::sleep(Duration::from_millis(20)) => {
                    self.cancel.check()?;
                    if deadline.is_some_and(|end| Instant::now() >= end) {
                        return Err(MikoError::unexpected("element search timed out"));
                    }
                }
            }
        }
    }

    async fn action(&mut self, action: &FlowAction, deadline: Option<Instant>) -> MikoResult<()> {
        self.check(deadline)?;
        let action_cancel = Cancel::new();
        let task = self.host.action(action, &action_cancel);
        tokio::pin!(task);
        loop {
            tokio::select! {
                result = &mut task => return result,
                () = tokio::time::sleep(Duration::from_millis(20)) => {
                    let result = self.cancel.check().and_then(|()| {
                        if deadline.is_some_and(|end| Instant::now() >= end) {
                            Err(MikoError::unexpected("flow timed out"))
                        } else { Ok(()) }
                    });
                    if let Err(error) = result {
                        action_cancel.cancel();
                        // Wait for the host to release any held inputs before returning.
                        if let Err(release_error) = task.await
                            && !miko_core::cancel::is_cancelled(&release_error)
                        {
                            return Err(release_error);
                        }
                        return Err(error);
                    }
                }
            }
        }
    }

    fn nodes<'a>(
        &'a mut self,
        nodes: &'a [FlowNode],
        prefix: String,
        deadline: Option<Instant>,
    ) -> Pin<Box<dyn Future<Output = MikoResult<()>> + Send + 'a>> {
        Box::pin(async move {
            for (index, node) in nodes.iter().enumerate() {
                self.check(deadline)?;
                let path = if prefix.is_empty() {
                    format!("{}", index + 1)
                } else {
                    format!("{}.{}", prefix, index + 1)
                };
                let started = Instant::now();
                let checkpoint = self.host.checkpoint(&path, self.cancel);
                let checkpoint = if let Some(end) = deadline {
                    match tokio::time::timeout_at(end, checkpoint).await {
                        Ok(result) => result,
                        Err(_) => Err(MikoError::unexpected("flow timed out")),
                    }
                } else {
                    checkpoint.await
                };
                let result = match checkpoint {
                    Ok(()) => self.node(node, &path, deadline).await,
                    Err(error) => Err(error),
                };
                self.logs.push(FlowLog {
                    node: path.clone(),
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    outcome: result
                        .as_ref()
                        .map_or_else(|e| e.message.clone(), |_| "ok".into()),
                });
                if result.is_err() && self.failed_node.is_none() {
                    self.failed_node = Some(path);
                }
                result?;
            }
            Ok(())
        })
    }

    async fn node(
        &mut self,
        node: &FlowNode,
        path: &str,
        deadline: Option<Instant>,
    ) -> MikoResult<()> {
        match node {
            FlowNode::Action {
                action: FlowAction::TapElement { locator },
            } => {
                let point = self
                    .find(locator, deadline)
                    .await?
                    .ok_or_else(|| MikoError::not_found("element not found"))?;
                self.action(&FlowAction::Tap { point }, deadline).await
            }
            FlowNode::Action { action } => self.action(action, deadline).await,
            FlowNode::Wait { ms } => self.sleep(Duration::from_millis(*ms), deadline).await,
            FlowNode::WaitElement {
                locator,
                present,
                timeout_ms,
                interval_ms,
            } => {
                let end = Instant::now() + Duration::from_millis(*timeout_ms);
                let end = deadline.map_or(end, |d| end.min(d));
                loop {
                    let found = self.find(locator, Some(end)).await?.is_some();
                    if found == *present {
                        return Ok(());
                    }
                    if Instant::now() >= end {
                        return Err(MikoError::unexpected("element wait timed out"));
                    }
                    self.sleep(
                        Duration::from_millis(*interval_ms)
                            .min(end.saturating_duration_since(Instant::now())),
                        Some(end),
                    )
                    .await?;
                }
            }
            FlowNode::Branch {
                locator,
                then_nodes,
                else_nodes,
            } => {
                let found = self.find(locator, deadline).await?.is_some();
                self.nodes(
                    if found { then_nodes } else { else_nodes },
                    path.to_owned(),
                    deadline,
                )
                .await
            }
            FlowNode::Retry {
                nodes,
                max_attempts,
                timeout_ms,
                interval_ms,
            } => {
                let end = Instant::now() + Duration::from_millis(*timeout_ms);
                let end = deadline.map_or(end, |d| end.min(d));
                for attempt in 0..*max_attempts {
                    self.failed_node = None;
                    let result = self.nodes(nodes, path.to_owned(), Some(end)).await;
                    if result.is_ok() {
                        self.failed_node = None;
                        return Ok(());
                    }
                    if result
                        .as_ref()
                        .err()
                        .and_then(|error| error.data.as_ref())
                        .and_then(|data| data.get("flow_fatal"))
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
                    {
                        return result;
                    }
                    self.cancel.check()?;
                    if attempt + 1 == *max_attempts || Instant::now() >= end {
                        return result;
                    }
                    self.sleep(
                        Duration::from_millis(*interval_ms)
                            .min(end.saturating_duration_since(Instant::now())),
                        Some(end),
                    )
                    .await?;
                }
                Err(MikoError::unexpected("retry exhausted"))
            }
        }
    }
}

pub async fn run_flow<H: FlowHost>(
    flow: &Flow,
    host: &mut H,
    cancel: &Cancel,
) -> MikoResult<FlowReport> {
    flow.validate()?;
    let start = Instant::now();
    let mut runner = Runner {
        host,
        cancel,
        logs: Vec::new(),
        failed_node: None,
    };
    let result = runner.nodes(&flow.nodes, String::new(), None).await;
    Ok(FlowReport {
        name: flow.name.clone(),
        elapsed_ms: start.elapsed().as_millis() as u64,
        logs: runner.logs,
        failed_node: runner.failed_node,
        error: result.err().map(|e| e.message),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    struct Host {
        attempts: usize,
        released: Arc<AtomicBool>,
    }
    #[async_trait]
    impl FlowHost for Host {
        async fn find(&mut self, _: &Locator) -> MikoResult<Option<Point>> {
            Ok(Some(Point { x: 0.5, y: 0.5 }))
        }
        async fn action(&mut self, action: &FlowAction, cancel: &Cancel) -> MikoResult<()> {
            self.attempts += 1;
            match action {
                FlowAction::Key { .. } if self.attempts == 1 => {
                    Err(MikoError::unexpected("temporary failure"))
                }
                FlowAction::Text { .. } => Err(MikoError::unexpected("injection failed")
                    .with_data(serde_json::json!({"flow_fatal": true}))),
                FlowAction::Swipe { .. } => {
                    while !cancel.is_cancelled() {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                    self.released.store(true, Ordering::SeqCst);
                    Err(miko_core::cancel::cancelled())
                }
                _ => Ok(()),
            }
        }
        async fn checkpoint(&mut self, _: &str, cancel: &Cancel) -> MikoResult<()> {
            cancel.check()
        }
    }

    #[tokio::test]
    async fn branch_and_retry_report_the_actual_outcome() {
        let locator = Locator::Ui {
            text: Some("Next".into()),
            resource_id: None,
        };
        let flow = Flow {
            name: "test".into(),
            nodes: vec![FlowNode::Branch {
                locator,
                then_nodes: vec![FlowNode::Retry {
                    nodes: vec![FlowNode::Action {
                        action: FlowAction::Key {
                            key: "Enter".into(),
                        },
                    }],
                    max_attempts: 2,
                    timeout_ms: 1000,
                    interval_ms: 1,
                }],
                else_nodes: vec![],
            }],
        };
        let mut host = Host {
            attempts: 0,
            released: Arc::new(AtomicBool::new(false)),
        };
        let report = run_flow(&flow, &mut host, &Cancel::new()).await.unwrap();
        assert_eq!(host.attempts, 2);
        assert!(report.error.is_none() && report.failed_node.is_none());
        assert_eq!(
            report.logs.iter().filter(|log| log.node == "1.1.1").count(),
            2
        );
    }

    #[tokio::test]
    async fn retry_deadline_waits_for_action_cleanup() {
        let point = Point { x: 0.5, y: 0.5 };
        let flow = Flow {
            name: "test".into(),
            nodes: vec![FlowNode::Retry {
                nodes: vec![FlowNode::Action {
                    action: FlowAction::Swipe {
                        from: point,
                        to: point,
                        duration_ms: 5000,
                    },
                }],
                max_attempts: 2,
                timeout_ms: 10,
                interval_ms: 1,
            }],
        };
        let released = Arc::new(AtomicBool::new(false));
        let mut host = Host {
            attempts: 0,
            released: released.clone(),
        };
        let report = run_flow(&flow, &mut host, &Cancel::new()).await.unwrap();
        assert!(released.load(Ordering::SeqCst));
        assert!(report.error.unwrap().contains("timed out"));
        assert_eq!(host.attempts, 1);
    }

    #[tokio::test]
    async fn input_failure_is_not_retried() {
        let flow = Flow {
            name: "test".into(),
            nodes: vec![FlowNode::Retry {
                nodes: vec![FlowNode::Action {
                    action: FlowAction::Text { text: "x".into() },
                }],
                max_attempts: 3,
                timeout_ms: 1000,
                interval_ms: 1,
            }],
        };
        let mut host = Host {
            attempts: 0,
            released: Arc::new(AtomicBool::new(false)),
        };
        let report = run_flow(&flow, &mut host, &Cancel::new()).await.unwrap();
        assert_eq!(host.attempts, 1);
        assert_eq!(report.failed_node.as_deref(), Some("1.1"));
    }
}
