//! How a log line looks. Formatters and layers only; what a line SAYS is
//! decided by the caller, and which values are legal by config.rs.

use std::fmt;
use std::str::FromStr;

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

use crate::span::FixedField;

#[derive(Default, Clone)]
pub(crate) struct ReqCtx {
    pub reqid: Option<String>,
    pub actor: Option<String>,
}

struct ReqVisitor<'a>(&'a mut ReqCtx);

impl ReqVisitor<'_> {
    fn set(&mut self, field: FixedField, value: String) {
        match field {
            FixedField::ReqId => self.0.reqid = Some(value),
            FixedField::Actor => self.0.actor = Some(value),
        }
    }
}

impl Visit for ReqVisitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        if let Ok(field) = FixedField::from_str(field.name()) {
            self.set(field, value.to_owned());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let Ok(field) = FixedField::from_str(field.name()) else {
            return;
        };
        let s = format!("{value:?}");
        let s = s
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(&s);
        self.set(field, s.to_owned());
    }
}

#[derive(Default)]
pub(crate) struct SpanFields(pub Vec<(&'static str, String)>);

struct AllFieldsVisitor<'a>(&'a mut SpanFields);

impl AllFieldsVisitor<'_> {
    fn put(&mut self, name: &'static str, value: String) {
        match self.0 .0.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = value,
            None => self.0 .0.push((name, value)),
        }
    }
}

impl Visit for AllFieldsVisitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field.name(), format!("{value:?}"));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field.name(), value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field.name(), value.to_string());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field.name(), value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field.name(), value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.put(field.name(), format!("{value:?}"));
    }
}

pub(crate) struct CaptureLayer;

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        let span = ctx.span(id).expect("span exists on new_span");
        let mut rc = ReqCtx::default();
        attrs.record(&mut ReqVisitor(&mut rc));
        span.extensions_mut().insert(rc);
        let mut sf = SpanFields::default();
        attrs.record(&mut AllFieldsVisitor(&mut sf));
        span.extensions_mut().insert(sf);
    }

    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        ctx: Context<'_, S>,
    ) {
        let span = ctx.span(id).expect("span exists on record");
        let mut ext = span.extensions_mut();
        if let Some(rc) = ext.get_mut::<ReqCtx>() {
            values.record(&mut ReqVisitor(rc));
        }
        if let Some(sf) = ext.get_mut::<SpanFields>() {
            values.record(&mut AllFieldsVisitor(sf));
        }
    }
}

pub(crate) struct HumanFormat;

impl<S, N> FormatEvent<S, N> for HumanFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();

        let ts = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|_| fmt::Error)?;
        let (reqid, actor) = lookup_ctx(ctx, event);

        let mut rendered = EventFields::default();
        event.record(&mut rendered);

        write!(
            writer,
            "{ts} {} {} {}:{} {} [{}] ",
            meta.level().as_str(),
            rendered.designator.as_deref().unwrap_or("-"),
            meta.file().unwrap_or("?"),
            meta.line().unwrap_or(0),
            reqid.as_deref().unwrap_or("-"),
            actor.as_deref().unwrap_or("-"),
        )?;

        write!(writer, "{}", rendered.message.as_deref().unwrap_or(""))?;
        for (name, value) in &rendered.fields {
            write!(writer, " {name}={value}")?;
        }

        let mut seen: Vec<&'static str> = rendered.fields.iter().map(|(n, _)| *n).collect();
        let mut retained: Vec<(&'static str, String)> = Vec::new();
        if let Some(scope) = ctx.event_scope() {
            for span in scope {
                let ext = span.extensions();
                if ext
                    .get::<ReqCtx>()
                    .map(|c| c.reqid.is_some())
                    .unwrap_or(false)
                {
                    continue;
                }
                if let Some(sf) = ext.get::<SpanFields>() {
                    for (name, value) in &sf.0 {
                        if !seen.contains(name) {
                            seen.push(name);
                            retained.push((name, value.clone()));
                        }
                    }
                }
            }
        }
        for (name, value) in retained.iter().rev() {
            write!(writer, " {name}={value}")?;
        }

        writeln!(writer)
    }
}

#[derive(Default)]
struct EventFields {
    designator: Option<String>,
    message: Option<String>,
    fields: Vec<(&'static str, String)>,
}

impl EventFields {
    fn put(&mut self, name: &'static str, value: String) {
        match name {
            MESSAGE => self.message = Some(value),
            n if n == crate::designator::FIELD => self.designator = Some(value),
            _ => self.fields.push((name, value)),
        }
    }
}

const MESSAGE: &str = "message";

impl Visit for EventFields {
    fn record_str(&mut self, field: &Field, value: &str) {
        let name = field.name();
        if name == MESSAGE || name == crate::designator::FIELD {
            self.put(name, value.to_owned());
        } else {
            self.put(name, format!("{value:?}"));
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field.name(), value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field.name(), value.to_string());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field.name(), value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field.name(), value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let name = field.name();
        let rendered = format!("{value:?}");
        if name == MESSAGE || name == crate::designator::FIELD {
            self.put(name, rendered.trim_matches('"').to_owned());
        } else {
            self.put(name, rendered);
        }
    }
}

fn lookup_ctx<S, N>(
    ctx: &FmtContext<'_, S, N>,
    event: &Event<'_>,
) -> (Option<String>, Option<String>)
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    let mut reqid = None;
    let mut actor = None;
    if let Some(scope) = ctx.event_scope() {
        for span in scope {
            if let Some(rc) = span.extensions().get::<ReqCtx>() {
                if reqid.is_none() {
                    reqid = rc.reqid.clone();
                }
                if actor.is_none() {
                    actor = rc.actor.clone();
                }
            }
        }
    }
    let _ = event;
    (reqid, actor)
}
