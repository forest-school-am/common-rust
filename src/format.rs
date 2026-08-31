//! The human output format (CODESTYLE.md §8.4) and the small layer that makes
//! it possible. tracing-subscriber's built-in formatters can't produce the
//! exact `timestamp level designator file:row reqid [actor] message` layout
//! with `reqid`/`actor` in fixed positions, so [`HumanFormat`] is a custom
//! `FormatEvent`. It needs the request span's `reqid`/`actor` as typed values
//! at format time, which the default field store doesn't expose per-name — so
//! [`CaptureLayer`] snapshots just those two into span extensions. (JSON uses
//! the stock JSON layer, which reads span fields itself; it needs neither.)

use std::fmt;

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields, FormattedFields};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

/// The two request-span fields we surface at fixed positions in the human
/// line. Stored per span by [`CaptureLayer`].
#[derive(Default, Clone)]
pub(crate) struct ReqCtx {
    pub reqid: Option<String>,
    pub actor: Option<String>,
}

struct ReqVisitor<'a>(&'a mut ReqCtx);

impl Visit for ReqVisitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "reqid" => self.0.reqid = Some(value.to_owned()),
            "actor" => self.0.actor = Some(value.to_owned()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        // fallback for %/? -recorded values: strip one layer of Debug quoting
        if matches!(field.name(), "reqid" | "actor") {
            let s = format!("{value:?}");
            let s = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(&s);
            match field.name() {
                "reqid" => self.0.reqid = Some(s.to_owned()),
                "actor" => self.0.actor = Some(s.to_owned()),
                _ => {}
            }
        }
    }
}

/// Snapshots `reqid`/`actor` from any span that declares them, so the human
/// formatter can place them precisely. Cheap: a per-span struct with two
/// `Option<String>`, only on spans that carry the fields.
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
    }
}

/// `timestamp level designator file:row reqid [actor] message`, one line per
/// event, with any nested-span fields appended at the end.
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

        // Single-space-separated fields (a fixed-width level would inject
        // variable spacing and break field-splitting).
        // timestamp level designator file:row reqid [actor]
        let ts = OffsetDateTime::now_utc().format(&Rfc3339).map_err(|_| fmt::Error)?;
        let (reqid, actor) = lookup_ctx(ctx, event);
        write!(
            writer,
            "{ts} {} {} {}:{} {} [{}] ",
            meta.level().as_str(),
            meta.target(),
            meta.file().unwrap_or("?"),
            meta.line().unwrap_or(0),
            reqid.as_deref().unwrap_or("-"),
            actor.as_deref().unwrap_or("-"),
        )?;

        // message + event fields
        ctx.format_fields(writer.by_ref(), event)?;

        // nested-span fields inline at the end (skip the request span, whose
        // reqid/actor are already surfaced above)
        if let Some(scope) = ctx.event_scope() {
            for span in scope.from_root() {
                let ext = span.extensions();
                if ext.get::<ReqCtx>().map(|c| c.reqid.is_some()).unwrap_or(false) {
                    continue;
                }
                if let Some(fields) = ext.get::<FormattedFields<N>>() {
                    if !fields.fields.is_empty() {
                        write!(writer, " {}", fields.fields.as_str())?;
                    }
                }
            }
        }

        writeln!(writer)
    }
}

/// Walk the event's span scope (leaf → root) and take the first `reqid`/
/// `actor` found.
fn lookup_ctx<S, N>(ctx: &FmtContext<'_, S, N>, event: &Event<'_>) -> (Option<String>, Option<String>)
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
