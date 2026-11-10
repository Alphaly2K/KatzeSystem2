use katze_system2::{
    Result,
    expression::State,
    graphics::ParticleObject,
    kcs::CoreHost,
    runtime::{TaskObject, kind},
    task::{Command, CommandBuffers},
};

#[test]
fn particle_controls_keep_signed_opacity_and_clear_zero_transform_sources() -> Result<()> {
    let mut particle = ParticleObject::new(7);
    let mut numbers = State::new(0);
    let mut host = CoreHost::new(&mut numbers);
    let command = |code, value| Command {
        target: 7,
        sender: 13,
        code,
        data: [0x1234, value, u32::MAX],
    };
    let mut output = 0xdead_beef;
    assert_eq!(particle.opacity(), 256);
    assert_eq!(particle.transform_source(), u32::MAX);
    for (value, opacity) in [
        (0, 0),
        (254, 254),
        (255, 256),
        (256, 256),
        (i32::MAX as u32, 256),
        (i32::MIN as u32, i32::MIN as u32),
        (u32::MAX, u32::MAX),
    ] {
        let outcome = particle.dispatch_command(
            kind::PARTICLE,
            &command(326, value),
            &mut output,
            &mut host,
            1,
        )?;
        assert!(outcome.done && !outcome.propagate);
        assert_eq!(particle.opacity(), opacity);
        assert_eq!(output, 0xdead_beef);
    }
    for value in [7, 0, i32::MIN as u32, u32::MAX] {
        let outcome = particle.dispatch_command_with_buffers(
            kind::PARTICLE,
            &command(327, value),
            &mut CommandBuffers::default(),
            &mut output,
            &mut host,
            1,
        )?;
        assert!(outcome.done && !outcome.propagate);
        assert_eq!(
            particle.transform_source(),
            if value == 0 { u32::MAX } else { value }
        );
        assert_eq!(output, 0xdead_beef);
    }
    for code in [320, 321, 322, 323, 324, 325, 591, 592] {
        assert!(
            particle
                .dispatch_command(kind::PARTICLE, &command(code, 1), &mut output, &mut host, 1)
                .is_err()
        );
    }
    particle.state().0.deletion_requested = 1;
    particle.dispatch_command(kind::PARTICLE, &command(326, 0), &mut output, &mut host, 1)?;
    particle.dispatch_command(kind::PARTICLE, &command(327, 1), &mut output, &mut host, 1)?;
    assert_eq!(particle.opacity(), u32::MAX);
    assert_eq!(particle.transform_source(), u32::MAX);
    assert_eq!(output, 0xdead_beef);
    Ok(())
}
