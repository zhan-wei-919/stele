use std::sync::{Mutex, MutexGuard, OnceLock};

use log::{LevelFilter, Log, Metadata, Record};

#[derive(Default)]
struct TestLogger {
    entries: Mutex<Vec<String>>,
}

impl Log for TestLogger {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &Record<'_>) {
        self.entries
            .lock()
            .expect("test logger entries must lock")
            .push(format!(
                "{} {} {}",
                record.level(),
                record.target(),
                record.args()
            ));
    }

    fn flush(&self) {}
}

static TEST_LOGGER: TestLogger = TestLogger {
    entries: Mutex::new(Vec::new()),
};
static TEST_LOGGER_INIT: OnceLock<()> = OnceLock::new();
static TEST_LOGGER_SERIAL: Mutex<()> = Mutex::new(());

pub(crate) struct LogCapture {
    _serial: MutexGuard<'static, ()>,
}

impl LogCapture {
    pub(crate) fn begin() -> Self {
        TEST_LOGGER_INIT.get_or_init(|| {
            log::set_logger(&TEST_LOGGER).expect("test logger must install once");
            log::set_max_level(LevelFilter::Trace);
        });

        let serial = TEST_LOGGER_SERIAL
            .lock()
            .expect("test logger serial lock must lock");
        TEST_LOGGER
            .entries
            .lock()
            .expect("test logger entries must lock")
            .clear();
        Self { _serial: serial }
    }

    pub(crate) fn contains(&self, needle: &str) -> bool {
        TEST_LOGGER
            .entries
            .lock()
            .expect("test logger entries must lock")
            .iter()
            .any(|entry| entry.contains(needle))
    }

    pub(crate) fn entries(&self) -> Vec<String> {
        TEST_LOGGER
            .entries
            .lock()
            .expect("test logger entries must lock")
            .clone()
    }
}
