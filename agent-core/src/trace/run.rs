use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::json;

use super::{TraceError, TraceEvent, TraceSink};
use crate::{agent::AgentError, model::ModelUsage};

static NEXT_RUN: AtomicU64 = AtomicU64::new(1);

struct Span {
    id: u64,
    name: &'static str,
    started: Instant,
    usage: ModelUsage,
    model_calls: u64,
    cache_hits: u64,
    cache_reports: u64,
}

/// 只保留活动节点栈，空间随调用深度增长，不保留整轮历史。
pub(crate) struct RunTrace {
    id: String,
    session_id: String,
    started: Instant,
    next_span: u64,
    stack: Vec<Span>,
}

impl RunTrace {
    pub(crate) fn new(session_id: &str) -> Self {
        Self {
            id: format!(
                "{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos(),
                NEXT_RUN.fetch_add(1, Ordering::Relaxed)
            ),
            session_id: session_id.to_owned(),
            started: Instant::now(),
            next_span: 0,
            stack: Vec::new(),
        }
    }

    pub(crate) fn start(
        &mut self,
        sink: &mut impl TraceSink,
        name: &'static str,
        details: serde_json::Value,
    ) -> Result<(), TraceError> {
        self.next_span += 1;
        let started = Instant::now();
        let parent = self.stack.last().map(|span| span.id);
        self.stack.push(Span {
            id: self.next_span,
            name,
            started,
            usage: ModelUsage::zero(),
            model_calls: 0,
            cache_hits: 0,
            cache_reports: 0,
        });
        self.record(
            sink,
            TraceEvent::new("trace.span.started")
                .with_field("operation", name)
                .with_field("parent_span_id", parent)
                .with_field("depth", (self.stack.len() - 1) as u64)
                .with_field(
                    "start_offset_ms",
                    started.duration_since(self.started).as_secs_f64() * 1000.0,
                )
                .with_field("details", details),
        )
    }

    pub(crate) fn record(
        &self,
        sink: &mut impl TraceSink,
        event: TraceEvent,
    ) -> Result<(), TraceError> {
        sink.record(
            event
                .with_field("run_id", self.id.as_str())
                .with_field("session_id", self.session_id.as_str())
                .with_field("span_id", self.stack.last().map(|span| span.id)),
        )
    }

    pub(crate) fn model_usage(&mut self, usage: ModelUsage) {
        if let Some(span) = self.stack.last_mut() {
            span.usage = usage;
            span.model_calls = 1;
            span.cache_reports = u64::from(usage.cache_hit().is_some());
            span.cache_hits = u64::from(usage.cache_hit() == Some(true));
        }
    }

    pub(crate) fn end(
        &mut self,
        sink: &mut impl TraceSink,
        error: Option<&AgentError>,
    ) -> Result<(), TraceError> {
        let Some(span) = self.stack.last() else {
            return Ok(());
        };
        let event = TraceEvent::new(if error.is_some() {
            "trace.span.failed"
        } else {
            "trace.span.completed"
        })
        .with_field("operation", span.name)
        .with_field("duration_ms", span.started.elapsed().as_secs_f64() * 1000.0)
        .with_field("status", if error.is_some() { "failed" } else { "ok" })
        .with_field("error", error.map(ToString::to_string))
        .with_field("usage", json!(span.usage))
        .with_field("total_tokens", span.usage.total_tokens())
        .with_field("model_calls", span.model_calls)
        .with_field("cache_hits", span.cache_hits)
        .with_field("cache_reports", span.cache_reports)
        .with_field("cache_read_percent", span.usage.cache_read_percent())
        .with_field(
            "cache_status",
            if span.model_calls == 0 {
                "not_applicable"
            } else if span.cache_reports < span.model_calls {
                "unknown"
            } else if span.cache_hits > 0 {
                "hit"
            } else {
                "miss"
            },
        );
        let recorded = self.record(sink, event);
        let span = self.stack.pop().expect("active span");
        if let Some(parent) = self.stack.last_mut() {
            parent.usage.add(span.usage);
            parent.model_calls += span.model_calls;
            parent.cache_hits += span.cache_hits;
            parent.cache_reports += span.cache_reports;
        }
        recorded
    }

    pub(crate) fn finish(
        &mut self,
        sink: &mut impl TraceSink,
        error: Option<&AgentError>,
    ) -> Result<(), TraceError> {
        let mut first_error = None;
        while !self.stack.is_empty() {
            if let Err(error) = self.end(sink, error) {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub(crate) fn call<R, E: Into<AgentError>>(
        &mut self,
        sink: &mut impl TraceSink,
        name: &'static str,
        operation: impl FnOnce() -> Result<R, E>,
    ) -> Result<R, AgentError> {
        self.start(sink, name, json!({}))?;
        let result = operation().map_err(Into::into);
        let recorded = self.end(sink, result.as_ref().err());
        // 保留业务错误，避免追踪写入失败掩盖原始原因。
        let value = result?;
        recorded?;
        Ok(value)
    }
}
