pub(crate) mod compile;
pub(crate) mod print;

use std::sync::Arc;

use firefly_util::diagnostics::{
    CodeMap, DiagnosticsConfig, DiagnosticsHandler, DisplayConfig, Emitter,
};

use firefly_session::Options;

pub(super) fn create_diagnostics_handler(
    options: &Options,
    codemap: Arc<CodeMap>,
    emitter: Option<Arc<dyn Emitter>>,
) -> Arc<DiagnosticsHandler> {
    let emitter = emitter.unwrap_or_else(|| default_emitter(&options));
    let config = DiagnosticsConfig {
        warnings_as_errors: options.warnings_as_errors,
        no_warn: options.no_warn,
        display: DisplayConfig::default(),
    };
    Arc::new(DiagnosticsHandler::new(config, codemap, emitter))
}

pub(super) fn default_emitter(options: &Options) -> Arc<dyn Emitter> {
    use firefly_util::diagnostics::{DefaultEmitter, NullEmitter};
    use firefly_util::error::Verbosity;

    match options.verbosity {
        Verbosity::Silent => Arc::new(NullEmitter::new(options.color)),
        _ => Arc::new(DefaultEmitter::new(options.color)),
    }
}

pub(super) fn abort_on_err<T, E>(_: E) -> T {
    use firefly_util::error::FatalError;

    FatalError.raise()
}
