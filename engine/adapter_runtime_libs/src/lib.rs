//! Brings a runtime library (GLFW now) next to the executables of a build (card 4-35).
//!
//! Used from a `build.rs` as a build dependency. A manifest ([`manifest`]) names the official
//! archive (https URL, SHA-256 of the archive, the member to take, SHA-256 of the file); [`obtain`]
//! downloads and unpacks it once into a per-user cache and checks both hashes before the file is
//! kept; [`plan`] decides where it goes (the profile folder, `deps` and `examples` of the target
//! folder, so a program and a test binary find it) and turns every problem into a warning: a build
//! never fails because a library could not be fetched.
//!
//! No crate is used: the hash is written here and checked against the standard test vectors, the
//! download and the unpacking are the `curl.exe` and `tar.exe` that Windows 10 and later ship.

pub mod manifest;
pub mod obtain;
pub mod plan;
pub mod sha256;

#[cfg(test)]
mod tests;
