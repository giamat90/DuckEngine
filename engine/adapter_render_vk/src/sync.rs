//! The meaning of a [`ResourceState`] of the render port in Vulkan terms: the image layout, and
//! the pipeline stage and access that touch the image in that state. One pure function, so the
//! mapping is a table that is tested on its own (design: `wiki/architecture/vulkan-and-mesh-v0.md`
//! section 3).

use crate::vk_draw::{
    ACCESS_COLOR_ATTACHMENT_READ, ACCESS_DEPTH_STENCIL_READ, ACCESS_DEPTH_STENCIL_WRITE,
    ACCESS_SHADER_READ, STAGE_EARLY_FRAGMENT_TESTS, STAGE_FRAGMENT_SHADER,
    STAGE_LATE_FRAGMENT_TESTS,
};
use crate::vk_present::{
    ACCESS_COLOR_ATTACHMENT_WRITE, ACCESS_NONE, LAYOUT_COLOR_ATTACHMENT, LAYOUT_PRESENT_SRC,
    LAYOUT_UNDEFINED, STAGE_ALL_COMMANDS, STAGE_COLOR_ATTACHMENT_OUTPUT, STAGE_NONE,
};
use crate::vk_resource::{LAYOUT_DEPTH_ATTACHMENT, LAYOUT_SHADER_READ};
use render::desc::ResourceState;

/// What a resource state means to the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateAccess {
    pub layout: u32,
    pub stage: u64,
    pub access: u64,
}

/// The layout, stage and access of `state`. `Undefined` waits for everything before it (nothing
/// is kept, so only the order matters) and `Present` is the end of the frame's work.
#[must_use]
pub fn state_access(resource: ResourceState) -> StateAccess {
    let (layout, stage, touch) = match resource {
        ResourceState::Undefined => (LAYOUT_UNDEFINED, STAGE_ALL_COMMANDS, ACCESS_NONE),
        ResourceState::ColorTarget => (
            LAYOUT_COLOR_ATTACHMENT,
            STAGE_COLOR_ATTACHMENT_OUTPUT,
            ACCESS_COLOR_ATTACHMENT_READ | ACCESS_COLOR_ATTACHMENT_WRITE,
        ),
        ResourceState::DepthTarget => (
            LAYOUT_DEPTH_ATTACHMENT,
            STAGE_EARLY_FRAGMENT_TESTS | STAGE_LATE_FRAGMENT_TESTS,
            ACCESS_DEPTH_STENCIL_READ | ACCESS_DEPTH_STENCIL_WRITE,
        ),
        ResourceState::ShaderRead => (
            LAYOUT_SHADER_READ,
            STAGE_FRAGMENT_SHADER,
            ACCESS_SHADER_READ,
        ),
        ResourceState::Present => (LAYOUT_PRESENT_SRC, STAGE_NONE, ACCESS_NONE),
    };
    StateAccess {
        layout,
        stage,
        access: touch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [ResourceState; 5] = [
        ResourceState::Undefined,
        ResourceState::ColorTarget,
        ResourceState::DepthTarget,
        ResourceState::ShaderRead,
        ResourceState::Present,
    ];

    #[test]
    fn every_state_has_its_own_layout() {
        let layouts: Vec<u32> = ALL.iter().map(|s| state_access(*s).layout).collect();
        assert_eq!(
            layouts,
            [
                LAYOUT_UNDEFINED,
                LAYOUT_COLOR_ATTACHMENT,
                LAYOUT_DEPTH_ATTACHMENT,
                LAYOUT_SHADER_READ,
                LAYOUT_PRESENT_SRC
            ]
        );
    }

    #[test]
    fn a_colour_target_is_written_at_the_colour_output_stage() {
        let access = state_access(ResourceState::ColorTarget);
        assert_eq!(access.stage, STAGE_COLOR_ATTACHMENT_OUTPUT);
        assert_ne!(access.access & ACCESS_COLOR_ATTACHMENT_WRITE, 0);
    }

    #[test]
    fn a_depth_target_is_tested_in_both_fragment_test_stages() {
        let access = state_access(ResourceState::DepthTarget);
        assert_eq!(
            access.stage,
            STAGE_EARLY_FRAGMENT_TESTS | STAGE_LATE_FRAGMENT_TESTS
        );
        assert_eq!(
            access.access,
            ACCESS_DEPTH_STENCIL_READ | ACCESS_DEPTH_STENCIL_WRITE
        );
    }

    #[test]
    fn a_sampled_texture_is_read_by_the_fragment_shader() {
        let access = state_access(ResourceState::ShaderRead);
        assert_eq!(
            (access.stage, access.access),
            (STAGE_FRAGMENT_SHADER, ACCESS_SHADER_READ)
        );
    }

    #[test]
    fn present_and_undefined_touch_nothing_and_wait_for_everything_before() {
        let present = state_access(ResourceState::Present);
        assert_eq!((present.stage, present.access), (STAGE_NONE, ACCESS_NONE));
        let undefined = state_access(ResourceState::Undefined);
        assert_eq!(
            (undefined.stage, undefined.access),
            (STAGE_ALL_COMMANDS, ACCESS_NONE)
        );
    }
}
