use katze_system2::host::model::{ModelConfig, decode_psz};
use std::io::Write;

fn pack(payload: &[u8]) -> Vec<u8> {
    let mut compressor = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
    compressor.write_all(payload).unwrap();
    let packed = compressor.finish().unwrap();
    let mut result = b"PSZ\0".to_vec();
    for word in [packed.len() as u32, payload.len() as u32, 0] {
        result.extend(word.to_le_bytes());
    }
    result.extend(packed);
    result
}

#[test]
fn psz_preserves_model_bytes_and_checks_lengths_before_decoding() {
    let model = b"PSB\0synthetic data for the host model decoder";
    let mut resource = pack(model);
    assert_eq!(decode_psz(&resource, model.len()).unwrap(), model);
    assert!(decode_psz(&resource, model.len() - 1).is_err());
    assert!(decode_psz(&resource[..15], model.len()).is_err());
    assert!(decode_psz(&pack(b"bad model bytes"), model.len()).is_err());

    let declared = (model.len() + 1) as u32;
    resource[8..12].copy_from_slice(&declared.to_le_bytes());
    assert!(decode_psz(&resource, model.len() + 1).is_err());
    resource[8..12].copy_from_slice(&(model.len() as u32).to_le_bytes());
    resource.push(0);
    assert!(decode_psz(&resource, model.len()).is_err());
    let extra = (resource.len() - 16) as u32;
    resource[4..8].copy_from_slice(&extra.to_le_bytes());
    assert!(decode_psz(&resource, model.len()).is_err());
}

#[test]
fn model_config_preserves_aliases_and_empty_motion_columns() {
    let bytes = b"// model table\r\n#CHARANAME\r\n1\t\tfirst.psz\r\nA\t\x82\xa0\tsecond.psz\r\n\r\n#motname\na\t0\t\x82\xa2\tstand\t\tmove\t\n#other\nunknown record\0ignored";
    let config = ModelConfig::parse(bytes).unwrap();
    assert_eq!(config.bodies.len(), 2);
    let body = config.body(b"a").unwrap();
    assert_eq!(
        (body.name, body.resource),
        (b"\x82\xa0".as_slice(), b"second.psz".as_slice())
    );
    assert!(config.body(b"1").unwrap().name.is_empty());
    let motion = config.motion(b"A", b"0").unwrap();
    assert_eq!(motion.label, b"\x82\xa2");
    assert_eq!(
        motion.names().collect::<Vec<_>>(),
        [b"stand".as_slice(), b"", b"move", b""]
    );
    assert!(config.motion(b"1", b"0").is_none());
    assert_eq!(
        ModelConfig::parse(b"#charaname\n1\tlabel")
            .unwrap_err()
            .offset,
        11
    );
}
