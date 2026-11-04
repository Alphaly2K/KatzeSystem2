use katze_system2::{
    Result,
    input::{Events, KeyEventKind, KeySample, KeyServices, KeyTransition, Keys, Regions},
    task::{Command, CommandBuffers},
};

fn command(keys: &mut Keys, code: u32, a: u32, b: u32) -> Result<bool> {
    let mut output = 0x13579bdf;
    let result = keys.dispatch_command(
        &Command {
            target: 8,
            sender: 9,
            code,
            data: [a, b, 0],
        },
        &mut output,
    );
    assert_eq!(output, 0x13579bdf);
    result
}

struct Services {
    samples: [[KeySample; 4]; 80],
    queries: Vec<(u32, u32)>,
    transitions: Vec<KeyTransition>,
    consume_first: bool,
}

impl Default for Services {
    fn default() -> Self {
        Self {
            samples: [[KeySample::default(); 4]; 80],
            queries: Vec::new(),
            transitions: Vec::new(),
            consume_first: false,
        }
    }
}

impl KeyServices for Services {
    fn sample(&mut self, key: u32, group: u32) -> Result<KeySample> {
        self.queries.push((key, group));
        Ok(self.samples[key as usize][group as usize])
    }

    fn dispatch(&mut self, _keys: &mut Keys, transition: KeyTransition) -> Result<bool> {
        self.transitions.push(transition);
        Ok(self.consume_first && transition.key == 0 && transition.group == 0)
    }
}

#[test]
fn consumed_changes_block_later_scans_until_the_repeat_phase() -> Result<()> {
    let mut keys = Keys::default();
    let mut services = Services {
        consume_first: true,
        ..Services::default()
    };
    services.samples[0][0] = KeySample { mask: 2, repeat: 1 };
    services.samples[1][0] = KeySample { mask: 1, repeat: 1 };
    for count in 1..30 {
        services.queries.clear();
        assert!(keys.scan(&mut services)?);
        assert_eq!(services.queries, [(0, 0)]);
        assert_eq!(keys.records()[0][0].count, count);
        assert_eq!(keys.records()[1][0].count, 0);
    }
    assert_eq!(services.transitions.len(), 1);
    assert_eq!(services.transitions[0].kind, KeyEventKind::Change);
    assert!(keys.scan(&mut services)?);
    assert_eq!(services.transitions.len(), 2);
    assert_eq!(services.transitions[1].kind, KeyEventKind::Repeat);
    assert_eq!(keys.records()[0][0].count, 0x80000000);
    services.samples[0][0].repeat = 0;
    for count in 1..5 {
        services.queries.clear();
        assert!(!keys.scan(&mut services)?);
        assert_eq!(keys.records()[0][0].count, 0x80000000 + count);
        assert_eq!(keys.records()[1][0].count, count);
        assert_eq!(services.queries.len(), 320);
        assert_eq!(&services.queries[..2], &[(0, 0), (1, 0)]);
        assert_eq!(services.queries[80], (0, 1));
        assert_eq!(services.queries[319], (79, 3));
    }
    services.queries.clear();
    assert!(keys.scan(&mut services)?);
    assert_eq!(services.queries, [(0, 0)]);
    assert_eq!(keys.records()[0][0].count, 0x80000000);
    assert_eq!(keys.records()[1][0].count, 4);
    assert_eq!(
        services.transitions.last().unwrap().kind,
        KeyEventKind::Repeat
    );
    services.samples[0][0].mask = 0;
    assert!(keys.scan(&mut services)?);
    assert_eq!(keys.records()[0][0].count, 0);
    assert_eq!(keys.records()[0][0].mask, 0);
    assert!(keys.records()[0][0].consumed);
    assert_eq!(services.transitions.last().unwrap().pressed, 0);
    assert!(!keys.scan(&mut services)?);
    assert_eq!(keys.records()[1][0].count, 5);
    Ok(())
}

#[test]
fn source_sixteen_restarts_changes_and_disabled_keys_freeze_all_groups() -> Result<()> {
    let mut keys = Keys::default();
    let mut services = Services::default();
    services.samples[0][0] = KeySample {
        mask: 16,
        repeat: 0,
    };
    services.samples[79][3] = KeySample { mask: 3, repeat: 0 };
    for _ in 0..35 {
        assert!(!keys.scan(&mut services)?);
    }
    let events: Vec<_> = services.transitions.iter().filter(|t| t.key == 0).collect();
    assert_eq!(events.len(), 35);
    assert!(
        events
            .iter()
            .all(|t| t.kind == KeyEventKind::Change && t.mask == 2)
    );
    assert_eq!(keys.records()[0][0].count, 1);
    assert_eq!(keys.records()[0][0].mask, 16);
    assert_eq!(keys.records()[79][3].count, 35);
    services.samples[79][3].repeat = 1;
    let before = services.transitions.len();
    keys.scan(&mut services)?;
    assert_eq!(services.transitions.len(), before + 1);
    assert!(
        services
            .transitions
            .iter()
            .all(|t| t.kind != KeyEventKind::Repeat)
    );
    assert!(command(&mut keys, 151, 0, 0)?);
    services.queries.clear();
    keys.scan(&mut services)?;
    assert_eq!(services.queries.len(), 316);
    assert!(services.queries.iter().all(|&(key, _)| key != 0));
    assert_eq!(keys.records()[0][0].count, 1);
    assert_eq!(keys.records()[0][0].mask, 16);
    let records = *keys.records();
    assert!(command(&mut keys, 152, u32::MAX, u32::MAX)?);
    assert_eq!(keys.enabled(), &[1; 80]);
    assert_eq!(keys.records(), &records);
    assert!(command(&mut keys, 151, 0, 5)?);
    assert_eq!(keys.enabled()[0], 5);
    assert!(command(&mut keys, 151, 80, 0).is_err());
    assert!(!command(&mut keys, 999, 0, 0)?);
    services.samples[0][0].mask = 3;
    keys.scan(&mut services)?;
    assert_eq!(keys.records()[0][0].count, 1);
    assert_eq!(keys.records()[0][0].mask, 3);
    assert!(
        services
            .transitions
            .iter()
            .any(|t| t.key == 0 && t.mask == 3 && t.pressed == 1)
    );
    Ok(())
}

struct Routed {
    events: Events,
    regions: Regions,
    mask: u32,
    log: Vec<Command>,
}

impl KeyServices for Routed {
    fn sample(&mut self, key: u32, group: u32) -> Result<KeySample> {
        Ok(KeySample {
            mask: if key == 0 && group == 0 { self.mask } else { 0 },
            repeat: 1,
        })
    }

    fn dispatch(&mut self, _keys: &mut Keys, transition: KeyTransition) -> Result<bool> {
        self.events
            .dispatch_transition(8, &mut self.regions, transition, [0; 2], |_, packet| {
                self.log.push(packet);
                Ok(Some(0))
            })
    }
}

#[test]
fn scan_transitions_route_message_four_and_clear_suppression_only_for_changes() -> Result<()> {
    let mut services = Routed {
        events: Events::default(),
        regions: Regions::default(),
        mask: 16,
        log: Vec::new(),
    };
    let mut bytes = [0; 60];
    bytes[..4].copy_from_slice(&7u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&2u32.to_le_bytes());
    services.regions.dispatch_command(
        &Command {
            sender: 9,
            target: 8,
            code: 127,
            data: [0; 3],
        },
        &mut CommandBuffers {
            data0: None,
            data1: Some(&mut bytes),
            external: None,
        },
        &mut 0,
    )?;
    let mut keys = Keys::default();
    services.events.set_suppressed(9);
    assert!(keys.scan(&mut services)?);
    assert_eq!(services.events.suppressed(), 0);
    assert_eq!(services.events.captures()[0], 7);
    assert_eq!(services.events.raw_state(), 2);
    assert_eq!(keys.records()[0][0].mask, 16);
    services.mask = 2;
    assert!(keys.scan(&mut services)?);
    for _ in 2..30 {
        assert!(keys.scan(&mut services)?);
    }
    services.events.set_suppressed(7);
    assert!(!keys.scan(&mut services)?);
    assert_eq!(keys.records()[0][0].count, 0x80000000);
    assert_eq!(services.events.suppressed(), 7);
    assert_eq!(services.log.len(), 2);
    services.mask = 0;
    assert!(keys.scan(&mut services)?);
    assert_eq!(services.events.suppressed(), 0);
    assert_eq!(services.events.captures()[0], 0);
    assert_eq!(services.log.len(), 3);
    assert_eq!(services.log.last().unwrap().data, [0, 0, 0]);
    assert!(
        services
            .log
            .iter()
            .all(|p| p.code == 4 && p.target == 7 && p.sender == 8)
    );
    Ok(())
}
