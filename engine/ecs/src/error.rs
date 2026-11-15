//! Errors of the ECS: values, never panics (digest section 4).

use core::fmt;

/// Why an ECS operation was refused. Every variant leaves the world as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcsError {
    /// The entity was despawned (or never existed): its generation no longer matches its slot.
    StaleEntity,
    /// The entity does not have the component that was asked to be removed.
    MissingComponent,
    /// More entities than a 32-bit slot index can name.
    TooManyEntities,
    /// More component types, or archetypes, than a 32-bit index can name.
    TooManyComponents,
    /// A column did not hold the type its component id says, or a row was out of range: an internal inconsistency, reported
    /// instead of panicking.
    StorageMismatch,
}

impl fmt::Display for EcsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::StaleEntity => "the entity was despawned",
            Self::MissingComponent => "the entity does not have that component",
            Self::TooManyEntities => "too many entities for a 32-bit slot index",
            Self::TooManyComponents => "too many component types or archetypes",
            Self::StorageMismatch => "the storage is inconsistent (wrong column type or row)",
        };
        f.write_str(text)
    }
}

impl std::error::Error for EcsError {}

#[cfg(test)]
mod tests {
    use super::EcsError;

    #[test]
    fn every_error_has_its_own_message() {
        let all = [
            EcsError::StaleEntity,
            EcsError::MissingComponent,
            EcsError::TooManyEntities,
            EcsError::TooManyComponents,
            EcsError::StorageMismatch,
        ];
        let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
        assert!(texts.iter().all(|t| !t.is_empty()));
        let mut sorted = texts;
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len());
    }
}
