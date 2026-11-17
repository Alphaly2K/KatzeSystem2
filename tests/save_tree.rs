use katze_system2::save::{StateNode, StateTree};

fn node(data: &[u8], children: Vec<StateNode>) -> StateNode {
    StateNode {
        data: data.to_vec(),
        children,
    }
}

#[test]
fn state_records_use_unaligned_relative_links_and_reverse_each_restored_sibling_chain() {
    let tree = StateTree {
        roots: vec![
            node(&[1, 2, 3], vec![node(&[4], vec![]), node(&[5, 6], vec![])]),
            node(&[7], vec![]),
        ],
    };
    let bytes = vec![
        42, 0, 0, 0, 15, 0, 0, 0, 3, 0, 0, 0, 1, 2, 3, 13, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 4, 0,
        0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 5, 6, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 7,
    ];
    assert_eq!(tree.to_record().unwrap(), bytes);
    let expected = StateTree {
        roots: vec![
            node(&[7], vec![]),
            node(&[1, 2, 3], vec![node(&[5, 6], vec![]), node(&[4], vec![])]),
        ],
    };
    assert_eq!(StateTree::from_record(&bytes).unwrap(), expected);
    let mut padded = bytes.clone();
    padded.extend([99; 17]);
    assert_eq!(StateTree::from_record(&padded).unwrap(), expected);
    assert_eq!(StateTree::default().to_record().unwrap(), []);
    assert_eq!(
        StateTree::from_record(&[0; 12]).unwrap(),
        StateTree {
            roots: vec![StateNode::default()]
        }
    );
}

#[test]
fn state_records_reject_truncated_links_payloads_and_excessive_depth() {
    assert!(StateTree::from_record(&[]).is_err());
    assert!(StateTree::from_record(&[0; 11]).is_err());
    for (field, value) in [
        (0, 12u32),
        (4, 12),
        (8, 1),
        (0, u32::MAX),
        (4, u32::MAX),
        (8, u32::MAX),
    ] {
        let mut bytes = [0; 12];
        bytes[field..field + 4].copy_from_slice(&value.to_le_bytes());
        assert!(StateTree::from_record(&bytes).is_err());
    }
    let mut roots = Vec::new();
    let mut bytes = Vec::new();
    for index in 0..257 {
        roots = vec![node(&[], roots)];
        for value in [0u32, if index < 256 { 12 } else { 0 }, 0] {
            bytes.extend(value.to_le_bytes());
        }
    }
    assert!(
        StateTree { roots }
            .to_record()
            .unwrap_err()
            .message
            .contains("深度")
    );
    assert!(
        StateTree::from_record(&bytes)
            .unwrap_err()
            .message
            .contains("深度")
    );
}
