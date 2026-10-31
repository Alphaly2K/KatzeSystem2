use katze_system2::resource::Category;

#[test]
fn category_resolution_keeps_directory_bypass_and_baseline_cp932_boundaries() {
    let category = Category {
        prefix: b"mot".to_vec(),
        extension: b"kcs".to_vec(),
    };
    for (input, expected) in [
        (b"sample".as_slice(), b"mot/sample.kcs".as_slice()),
        (b"sample.KCS", b"mot/sample.KCS"),
        (b"sample.", b"mot/sample."),
        (b".sample", b"mot/.sample"),
        (b"folder/sample", b"folder/sample"),
        (b"folder\\sample", b"folder\\sample"),
        (b"/sample", b"mot//sample.kcs"),
        (b"C:sample", b"mot/C:sample.kcs"),
        (b"C:\\sample", b"C:\\sample"),
        (b"a\x81\x5cb", b"mot/a\x81\x5cb.kcs"),
        (b"a.\x81\x5cb", b"mot/a.\x81\x5cb.kcs"),
        (b"a\xf0\x5cb", b"a\xf0\x5cb"),
        (b"sample\0ignored", b"mot/sample.kcs"),
        (b"", b"mot/.kcs"),
    ] {
        assert_eq!(category.resolve(input).unwrap(), expected);
    }
    let category = Category {
        prefix: b"PFX/\0ignored".to_vec(),
        extension: b".kcs\0ignored".to_vec(),
    };
    assert_eq!(category.resolve(b"sample").unwrap(), b"PFX//sample..kcs");
    assert_eq!(Category::default().resolve(b"sample").unwrap(), b"sample.");
}
