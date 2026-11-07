//! The stability scenes and property tests (card 4-17): the scenario scenes of the phase 4 exit
//! criterion ("stacking/chain stability scenes pass"), each with bounds written down before they
//! were measured, run under every execution configuration, with sleeping, and over seeded random
//! scenes. One test binary, split by topic; `scene` holds the builders every module shares.

mod catalogue;
mod chains;
mod execution;
mod findings;
mod generator;
mod limits;
mod mass_ratio;
mod measure;
mod mixed;
mod properties;
mod pyramids;
mod scene;
mod sleeping;
mod stacks;
