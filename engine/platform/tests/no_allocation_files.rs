//! Starting, polling and taking background reads makes no allocator call on the caller's thread.

use base::alloc::{CountingAllocator, count_allocations};
use platform::files::{IFileSystem, ReadState};
use platform::memory_files::MemoryFileSystem;
use platform::std_files::StdFileSystem;
use platform::vpath::VPath;
use std::alloc::System;

const FIXTURE_PATH: &str = "test/hello.txt";
const FIXTURE_BYTES: &[u8] = b"hello, duck";

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

fn path() -> VPath {
    VPath::new(FIXTURE_PATH).unwrap_or_else(|_| panic!("valid path"))
}

/// One read cycle with a buffer and path made before the count starts.
fn cycle(fs: &dyn IFileSystem, path: VPath, buf: Box<[u8]>) -> Box<[u8]> {
    let id = fs.read_start(path, buf).unwrap_or_else(|e| panic!("{e}"));
    while fs.read_poll(id) == ReadState::Pending {
        std::thread::yield_now();
    }
    fs.read_take(id).unwrap_or_else(|| panic!("no buffer"))
}

fn assert_cycles_do_not_allocate(fs: &dyn IFileSystem) {
    // Warm up: the first read starts the I/O thread of the std file system.
    let buf = cycle(fs, path(), vec![0_u8; 32].into_boxed_slice());
    let paths: Vec<VPath> = (0..10).map(|_| path()).collect();
    let (buf, counts) = count_allocations(|| {
        let mut buf = buf;
        for path in paths {
            buf = cycle(fs, path, buf);
        }
        buf
    });
    // Freeing the paths made before the count is expected; getting new memory is not.
    assert_eq!(counts.allocations + counts.reallocations, 0);
    assert_eq!(&buf[..FIXTURE_BYTES.len()], FIXTURE_BYTES);
}

#[test]
fn the_memory_file_system_does_not_allocate_per_read() {
    let fs = MemoryFileSystem::new();
    assert_eq!(fs.add_file(FIXTURE_PATH, FIXTURE_BYTES), Ok(()));
    assert_cycles_do_not_allocate(&fs);
}

#[test]
fn the_std_file_system_does_not_allocate_per_read() {
    let dir = std::env::temp_dir().join(format!("duck-alloc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(dir.join("hello.txt"), FIXTURE_BYTES).unwrap_or_else(|e| panic!("{e}"));
    let fs = StdFileSystem::new();
    assert_eq!(fs.mount_dir("test", &dir), Ok(()));
    assert_cycles_do_not_allocate(&fs);
    drop(fs);
    let _ = std::fs::remove_dir_all(&dir);
}
