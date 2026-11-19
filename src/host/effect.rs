use crate::Result;

/// Creates a player for each scripted effect task. The host advances and renders it.
pub trait EffectServices {
    fn create_player(&mut self, task_id: u32) -> Result<Option<Box<dyn EffectPlayer>>>;
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
