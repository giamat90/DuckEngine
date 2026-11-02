//! Runtime asset formats and their readers. Layer 2: no operating-system access; the bytes come
//! from the file system port. Design: `wiki/architecture/render-port-v0.md` section 8 (D-6).

pub mod dshd;
pub mod dtex;
