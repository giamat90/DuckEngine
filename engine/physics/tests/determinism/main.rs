//! The determinism harness (card 4-18): the replay recorder, the execution configurations, the
//! scenes, and the tests that run every scene under every configuration (D0: the same run twice;
//! D1: other worker counts, schedulers, orders and chunk sizes) and compare the state hash and the
//! bit dump of every body after every step. Nothing is stored; the golden hash is card 4-20.

mod configs;
mod contract;
mod faulty;
mod matrix;
mod recorder;
mod scenes;
mod selftest;
mod shape;
