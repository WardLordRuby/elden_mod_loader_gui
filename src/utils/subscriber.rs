use tracing_subscriber::{filter::LevelFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(not(debug_assertions))]
pub fn init_subscriber() -> std::io::Result<Option<tracing_appender::non_blocking::WorkerGuard>> {
    use crate::{Cfg, Config, INI_NAME, LOG_NAME};
    use tracing_subscriber::fmt::format::PrettyFields;

    let env_dir = std::env::current_dir()?;

    if Cfg::read(&env_dir.join(INI_NAME))
        .map(|cfg| cfg.get_save_log().unwrap_or(true))
        .unwrap_or(true)
    {
        let file = std::fs::File::create(env_dir.join(LOG_NAME))?;
        let (non_blocking, guard) = tracing_appender::non_blocking(file);
        tracing_subscriber::registry()
            .with(
                fmt::layer()
                    .with_target(false)
                    .with_ansi(false)
                    .without_time()
                    .fmt_fields(PrettyFields::new())
                    .with_writer(non_blocking),
            )
            .with(LevelFilter::INFO)
            .init();
        return Ok(Some(guard));
        // MARK: TODO
        // create custom formatter to make the Panic messages print pretty
    }
    Ok(None)
}

#[cfg(debug_assertions)]
pub fn init_subscriber() -> std::io::Result<Option<()>> {
    use tracing_subscriber::filter::EnvFilter;

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
