//! The determinism harness (card 4-18): the replay recorder, the execution configurations, the
//! scenes, and the tests that run every scene under every configuration (D0: the same run twice;
//! D1: other worker counts, schedulers, orders and chunk sizes) and compare the state hash and the
//! bit dump of every body after every step. Nothing is stored here; the module `golden` (card 4-20)
//! prints the hashes that `gates/golden/physics-replay.toml` stores.

mod configs;
mod contract;
mod faulty;
mod golden;
mod matrix;
mod recorder;
mod scenes;
mod selftest;
mod shape;
