use katze_system2::host::model::decode_psz;
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
