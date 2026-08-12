//! File-size policy shared by the open command and its tests.

/// Five MiB is the inclusive editable boundary. A file exactly this size is
/// still editable; only a larger file is opened read-only.
pub const MAX_EDITABLE_BYTES: u64 = 5 * 1024 * 1024;
/// Files through 64 MiB may be inspected read-only. Larger files are rejected
/// before allocation so an accidental huge-file open cannot exhaust memory.
pub const MAX_OPENABLE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_FILE_BYTES: u64 = MAX_OPENABLE_BYTES;

pub const fn is_large(size: u64) -> bool {
    size > MAX_EDITABLE_BYTES
}

pub const fn read_only(size: u64) -> bool {
    is_large(size)
}

pub const fn should_disable_highlighting(size: u64) -> bool {
    is_large(size)
}

pub const fn should_reject_open(size: u64) -> bool {
    size > MAX_OPENABLE_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_mib_boundary_is_inclusive() {
        assert!(!is_large(0));
        assert!(!is_large(MAX_EDITABLE_BYTES - 1));
        assert!(!is_large(MAX_EDITABLE_BYTES));
        assert!(is_large(MAX_EDITABLE_BYTES + 1));
        assert!(read_only(MAX_EDITABLE_BYTES + 1));
        assert!(should_disable_highlighting(MAX_EDITABLE_BYTES + 1));
        assert!(!should_reject_open(MAX_OPENABLE_BYTES));
        assert!(should_reject_open(MAX_OPENABLE_BYTES + 1));
    }
}
