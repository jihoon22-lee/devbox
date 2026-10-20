use crate::{host::ProcFs, system::*};
use std::{fs, sync::Arc};
#[test]
fn system_memory_psi_and_missing_optional_data() {
    let dir = tempfile::tempdir().expect("fixture result");
    fs::create_dir(dir.path().join("pressure")).expect("fixture result");
    fs::write(
        dir.path().join("meminfo"),
        "MemTotal: 1000 kB\nMemAvailable: 600 kB\nSwapTotal: 100 kB\nSwapFree: 25 kB\n",
    )
    .expect("fixture result");
    fs::write(
        dir.path().join("pressure/cpu"),
        "some avg10=1.00 avg60=2.00 avg300=3.00 total=1234\n",
    )
    .expect("fixture result");
    let proc = ProcFs::fixture(dir.path().into(), Arc::new(|| 1), 100, 4096);
    let stats = system_snapshot(&proc, &dir.path().join("missing")).expect("fixture result");
    assert_eq!(stats.memory_total_bytes, 1_024_000);
    assert_eq!(stats.memory_available_bytes, 614_400);
    assert_eq!(stats.swap_used_bytes, 76_800);
    assert_eq!(
        stats
            .cpu_pressure
            .expect("fixture value")
            .some
            .expect("fixture value")
            .total_us,
        1234
    );
    assert!(stats.memory_pressure.is_none());
    assert!(stats.home_filesystem.is_none());
    assert!(parse_pressure("some avg10=NaN avg60=0 avg300=0 total=0").is_none());
    let live_fs = filesystem(dir.path()).expect("fixture value");
    assert!(live_fs.total_bytes > 0);
    assert!(live_fs.available_bytes <= live_fs.total_bytes);
}
