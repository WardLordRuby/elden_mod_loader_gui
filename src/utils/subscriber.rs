use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(not(debug_assertions))]
use tracing::{Event, Level, Subscriber};

#[cfg(not(debug_assertions))]
use tracing_subscriber::{
    fmt::{
        format::{FormatEvent, FormatFields, PrettyFields, Writer},
        FmtContext,
    },
    registry::LookupSpan,
};

#[cfg(not(debug_assertions))]
struct CustomFormatter<E> {
    inner: E,
}

#[cfg(not(debug_assertions))]
impl<E> CustomFormatter<E> {
    fn new(inner: E) -> Self {
        Self { inner }
    }
}

#[cfg(not(debug_assertions))]
impl<S, N, E> FormatEvent<S, N> for CustomFormatter<E>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
    E: FormatEvent<S, N>,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let meta = event.metadata();
        if meta.level() == &Level::ERROR && meta.name() == "PANIC" {
            ctx.field_format().format_fields(writer.by_ref(), event)?;
            writeln!(writer)
        } else {
            self.inner.format_event(ctx, writer.by_ref(), event)
        }
    }
}

#[cfg(not(debug_assertions))]
pub fn init_subscriber() -> std::io::Result<Option<tracing_appender::non_blocking::WorkerGuard>> {
    use crate::{Cfg, Config, INI_NAME, LOG_NAME};

    let env_dir = std::env::current_dir()?;
    let log_dir = env_dir.join(LOG_NAME);

    if Cfg::read(&env_dir.join(INI_NAME))
        .map(|cfg| cfg.get_save_log().unwrap_or(true))
        .unwrap_or(true)
    {
        let file = std::fs::File::create(log_dir)?;
        let (non_blocking, guard) = tracing_appender::non_blocking(file);
        tracing_subscriber::registry()
            .with(
                fmt::layer()
                    .event_format(CustomFormatter::new(
                        fmt::format().with_target(false).with_ansi(false).without_time(),
                    ))
                    .fmt_fields(PrettyFields::new())
                    .with_writer(non_blocking),
            )
            .init();
        return Ok(Some(guard));
    } else if matches!(log_dir.try_exists(), Ok(true)) {
        std::fs::remove_file(log_dir)?;
    }
    Ok(None)
}

#[cfg(debug_assertions)]
pub fn init_subscriber() -> std::io::Result<Option<()>> {
    use tracing_subscriber::filter::{EnvFilter, LevelFilter};

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(false).pretty())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();
    Ok(None)
}