//! Playback services for external effect formats.

use crate::Result;

mod particle;
pub use particle::{
    ParticleContext, ParticleFrame, ParticlePlayer, ParticleSource, ParticleVertex,
};

/// Supplies implementations for external effect formats used by the game.
pub trait EffectServices {
    /// Create a player for a scripted effect task. The host advances and renders it.
    fn create_player(&mut self, _task_id: u32) -> Result<Option<Box<dyn EffectPlayer>>> {
        Ok(None)
    }

    /// Open a particle plugin that the runtime does not recognize. The name and
    /// bytes come from the configured resource service. Return `None` if the
    /// plugin is unsupported. The runtime never executes the supplied bytes.
    fn open_particle(
        &mut self,
        _task_id: u32,
        _name: &[u8],
        _bytes: &[u8],
    ) -> Result<Option<Box<dyn ParticlePlayer>>> {
        Ok(None)
    }
}

/// One task's effect asset and instances. Instances retain their asset independently.
/// Dropping a player releases its remaining backend resources.
pub trait EffectPlayer {
    /// Load the effect at the formatted resource name. False means unavailable.
    fn load(&mut self, name: &[u8]) -> Result<bool>;
    fn unload(&mut self);
    /// Start the current asset at a position in backend coordinates.
    fn play(&mut self, position: [f32; 3]) -> Option<u32>;
    fn stop(&mut self, instance: u32);
    fn is_playing(&mut self, instance: u32) -> bool;
    fn set_color(&mut self, instance: u32, rgba: [u8; 4]);
    fn set_location(&mut self, instance: u32, position: [f32; 3]);
    fn set_rotation(&mut self, instance: u32, rotation: [f32; 3]);
    /// Update the task's draw entry, selecting source geometry when requested.
    fn set_base_point(&mut self, point: [f32; 3], source: bool);
    /// Set the effect renderer's row-major projection and view matrices.
    fn set_camera(&mut self, projection: [f32; 16], view: [f32; 16]);
}
